//! WebDAV HTTP 客户端
//! 封装 PROPFIND / PUT / GET / MKCOL 操作

use anyhow::{anyhow, Result};
use log::{debug, error, info};
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, Method, StatusCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// WebDAV 客户端，持有连接信息和 reqwest 实例
#[derive(Clone)]
pub struct WebDavClient {
    pub base_url: String,
    pub username: String,
    pub password: String,
    pub client: Client,
}

impl WebDavClient {
    /// 创建客户端，默认请求超时 120 秒
    pub fn new(base_url: &str, username: &str, password: &str) -> Result<Self> {
        debug!("WebDavClient::new base_url={}", base_url);
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;
        Ok(Self {
            base_url: format!("{}/", base_url.trim_end_matches('/')),
            username: username.to_string(),
            password: password.to_string(),
            client,
        })
    }

    /// 将相对路径拼接到 base_url，返回完整 URL
    pub fn url_for(&self, remote_path: &str) -> Result<String> {
        let remote = remote_path.trim_start_matches('/');
        let base = url::Url::parse(&self.base_url)?;
        Ok(base.join(remote)?.to_string())
    }

    /// 递归创建远端目录（逐段 MKCOL，已存在时忽略 405/409）
    pub async fn mkcol_recursive(&self, remote_path: &str) -> Result<()> {
        debug!("[MKCOL] recursive path={}", remote_path);
        let mut current = String::new();
        for segment in remote_path.trim_matches('/').split('/') {
            if segment.is_empty() { continue; }
            if !current.is_empty() { current.push('/'); }
            current.push_str(segment);
            let url = self.url_for(&current)?;
            debug!("[MKCOL] → {}", url);
            let resp = self.client
                .request(Method::from_bytes(b"MKCOL")?, url.clone())
                .basic_auth(&self.username, Some(&self.password))
                .send().await?;
            let status = resp.status();
            debug!("[MKCOL] ← {} {}", status.as_u16(), url);
            if !(status.is_success()
                || status == StatusCode::METHOD_NOT_ALLOWED
                || status == StatusCode::CONFLICT)
            {
                error!("[MKCOL] failed: {} for {}", status, url);
                return Err(anyhow!("MKCOL failed: {}", status));
            }
        }
        Ok(())
    }

    /// 上传字节数据到指定远端路径
    pub async fn put_bytes(&self, remote_path: &str, body: Vec<u8>) -> Result<()> {
        let url = self.url_for(remote_path)?;
        info!("[PUT] → {} ({} bytes)", url, body.len());
        let resp = self.client.put(url.clone())
            .basic_auth(&self.username, Some(&self.password))
            .body(body).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            error!("[PUT] ← {} {} body={}", status.as_u16(), url, err_body);
            return Err(anyhow!("PUT failed: {}", status));
        }
        debug!("[PUT] ← {} {}", status.as_u16(), url);
        Ok(())
    }

    /// 流式上传，通过回调实时报告进度，支持取消
    pub async fn put_bytes_with_progress(
        &self,
        remote_path: &str,
        body: Vec<u8>,
        cancel_flag: Arc<AtomicBool>,
        on_progress: impl Fn(u64, u64) + Send + Sync + 'static,
    ) -> Result<()> {
        use futures_util::stream;

        let url = self.url_for(remote_path)?;
        let total = body.len() as u64;
        info!("[PUT+progress] → {} ({} bytes)", url, total);

        let chunk_size = 64 * 1024usize;
        let cancel = cancel_flag.clone();
        let mut sent = 0u64;

        let byte_stream = stream::iter(
            (0..body.len()).step_by(chunk_size).map(move |offset| {
                if cancel.load(Ordering::Relaxed) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "Task cancelled",
                    ));
                }
                let end = (offset + chunk_size).min(body.len());
                let chunk = bytes::Bytes::copy_from_slice(&body[offset..end]);
                sent += chunk.len() as u64;
                on_progress(sent, total);
                Ok(chunk)
            }),
        );

        let upload = self.client.put(&url)
            .basic_auth(&self.username, Some(&self.password))
            .header("Content-Length", total.to_string())
            .body(reqwest::Body::wrap_stream(byte_stream))
            .send();

        // 同时轮询取消标志
        let resp = tokio::select! {
            r = upload => r?,
            _ = poll_cancel(cancel_flag) => {
                return Err(anyhow!("Task cancelled"));
            }
        };

        let status = resp.status();
        if !status.is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            error!("[PUT+progress] ← {} {} body={}", status.as_u16(), url, err_body);
            return Err(anyhow!("PUT failed: {}", status));
        }
        debug!("[PUT+progress] ← {} {}", status.as_u16(), url);
        Ok(())
    }

    /// 下载远端路径的全部字节
    pub async fn get_bytes(&self, remote_path: &str) -> Result<Vec<u8>> {
        let url = self.url_for(remote_path)?;
        info!("[GET] → {}", url);
        let resp = self.client.get(url.clone())
            .basic_auth(&self.username, Some(&self.password))
            .send().await?;
        let status = resp.status();
        if !status.is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            error!("[GET] ← {} {} body={}", status.as_u16(), url, err_body);
            return Err(anyhow!("GET failed: {}", status));
        }
        let bytes = resp.bytes().await?.to_vec();
        info!("[GET] ← {} {} ({} bytes)", status.as_u16(), url, bytes.len());
        Ok(bytes)
    }

    /// 流式下载，通过回调报告进度 (bytes_done, bytes_total)
    pub async fn get_bytes_with_progress(
        &self,
        remote_path: &str,
        on_progress: impl Fn(u64, u64),
    ) -> Result<Vec<u8>> {
        let url = self.url_for(remote_path)?;
        info!("[GET] → {}", url);
        let resp = self.client.get(url.clone())
            .basic_auth(&self.username, Some(&self.password))
            .send().await?;
        let status = resp.status();
        if !status.is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            error!("[GET] ← {} {} body={}", status.as_u16(), url, err_body);
            return Err(anyhow!("GET failed: {}", status));
        }
        let total = resp.content_length().unwrap_or(0);
        let mut buf = Vec::with_capacity(total as usize);
        let mut stream = resp.bytes_stream();
        use futures_util::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            buf.extend_from_slice(&chunk);
            on_progress(buf.len() as u64, total);
        }
        info!("[GET] ← {} {} ({} bytes)", status.as_u16(), url, buf.len());
        Ok(buf)
    }

    /// 删除远端路径（文件或目录）
    pub async fn delete(&self, remote_path: &str) -> Result<()> {
        let url = self.url_for(remote_path)?;
        info!("[DELETE] → {}", url);
        let resp = self.client.delete(url.clone())
            .basic_auth(&self.username, Some(&self.password))
            .send().await?;
        let status = resp.status();
        if !status.is_success() && status != StatusCode::NOT_FOUND {
            let err_body = resp.text().await.unwrap_or_default();
            error!("[DELETE] ← {} {} body={}", status.as_u16(), url, err_body);
            return Err(anyhow!("DELETE failed: {}", status));
        }
        info!("[DELETE] ← {} {}", status.as_u16(), url);
        Ok(())
    }

    /// 发送 PROPFIND 请求，返回响应 XML 文本
    pub async fn propfind(&self, remote_path: &str, depth: u8) -> Result<String> {
        let url = self.url_for(remote_path)?;
        let body = r#"<?xml version="1.0" encoding="utf-8" ?><D:propfind xmlns:D="DAV:"><D:allprop/></D:propfind>"#;
        info!("[PROPFIND] → {} depth={} body={}", url, depth, body);
        let mut headers = HeaderMap::new();
        headers.insert("Depth", HeaderValue::from_str(&depth.to_string())?);
        let resp = self.client
            .request(Method::from_bytes(b"PROPFIND")?, url.clone())
            .basic_auth(&self.username, Some(&self.password))
            .headers(headers).body(body).send().await?;
        let status = resp.status();
        if !(status.is_success() || status == StatusCode::MULTI_STATUS) {
            let err_body = resp.text().await.unwrap_or_default();
            error!("[PROPFIND] ← {} {} body={}", status.as_u16(), url, err_body);
            return Err(anyhow!("PROPFIND failed: {}", status));
        }
        let text = resp.text().await?;
        info!("[PROPFIND] ← {} {} ({} chars)", status.as_u16(), url, text.len());
        debug!("[PROPFIND] response body:\n{}", text);
        Ok(text)
    }

    /// 列出指定路径下的直接子目录名（depth=1，过滤自身）
    pub async fn list_child_dirs(&self, remote_path: &str) -> Result<Vec<String>> {
        debug!("list_child_dirs remote_path={}", remote_path);
        let xml = self.propfind(remote_path, 1).await?;
        let hrefs = parse_href_list(&xml)?;
        let full_url = self.url_for(remote_path)?;
        let parent_raw = url::Url::parse(&full_url)
            .map(|u| u.path().trim_end_matches('/').to_string())
            .unwrap_or_else(|_| format!("/{}", remote_path.trim_matches('/')));
        let parent = percent_decode(&parent_raw);
        let prefix = format!("{}/", parent);
        debug!("list_child_dirs parent={}, prefix={}, hrefs={:?}", parent, prefix, hrefs);
        let mut out = Vec::new();
        for href in &hrefs {
            let path = href_to_path(href);
            let p = percent_decode(path.trim_end_matches('/'));
            if p == parent { continue; }
            if let Some(remain) = p.strip_prefix(&prefix) {
                if !remain.contains('/') && !remain.is_empty() {
                    out.push(remain.to_string());
                }
            } else {
                debug!("list_child_dirs skip href={} decoded={} (no prefix match)", href, p);
            }
        }
        out.sort();
        out.dedup();
        debug!("list_child_dirs result={:?}", out);
        Ok(out)
    }
}

/// 轮询取消标志，每 200ms 检查一次，标志为 true 时返回
async fn poll_cancel(flag: Arc<AtomicBool>) {
    loop {
        if flag.load(Ordering::Relaxed) { return; }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// 从 PROPFIND XML 响应中提取所有 `<href>` 值
fn parse_href_list(xml: &str) -> Result<Vec<String>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_href = false;
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => { if e.name().as_ref().ends_with(b"href") { in_href = true; } }
            Ok(Event::End(e))   => { if e.name().as_ref().ends_with(b"href") { in_href = false; } }
            Ok(Event::Text(t)) if in_href => { out.push(String::from_utf8_lossy(t.as_ref()).into_owned()); }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("Failed to parse PROPFIND XML: {e}")),
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// 从完整 URL 或路径字符串中提取 path 部分
fn href_to_path(href: &str) -> String {
    if let Ok(url) = url::Url::parse(href) {
        return url.path().to_string();
    }
    href.to_string()
}

/// 对 URL 编码的路径进行解码（处理中文目录名等）
fn percent_decode(s: &str) -> String {
    let bytes: Vec<u8> = s.bytes().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(
                &String::from_utf8_lossy(&bytes[i + 1..i + 3]),
                16,
            ) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}
