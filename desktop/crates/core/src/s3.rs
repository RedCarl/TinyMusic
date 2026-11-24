//! 极简 S3 客户端：SigV4 签名 + PUT Object + ListObjectsV2。
//!
//! 不引 aws-sdk（体积太大），只做网盘下载需要的三件事：
//! 上传对象、列对象、连通性探测。SHA-256 / HMAC-SHA256 用 `sha2` / `hmac`。
//!
//! 兼容 AWS S3、MinIO、Cloudflare R2、阿里云 OSS 的 S3 兼容端点等。
//! Path-Style 与 Virtual-Hosted-Style 两种寻址都支持。

use crate::error::{Error, Result};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// 超过这个体积改走分片上传。反向代理（openresty / nginx）常把
/// `client_max_body_size` 限制在 50MB，整包 PUT 会被 413 挡掉。
const MULTIPART_THRESHOLD: usize = 24 * 1024 * 1024;

/// 单个分片大小：留足余量，任何常见的 50MB 反代上限都不会碰到。
const MULTIPART_PART_SIZE: usize = 16 * 1024 * 1024;

/// 分片能缩到的下限。S3 协议要求「除最后一片外每片不小于 5MB」，
/// 反代上限比这还小的话，分片也救不了，只能让用户去调服务端。
const MULTIPART_MIN_PART_SIZE: usize = 5 * 1024 * 1024;

/// 网盘配置（全部来自设置页）
#[derive(Debug, Clone, Default)]
pub struct S3Config {
    /// 形如 `https://s3.us-east-1.amazonaws.com` 或 `http://127.0.0.1:9000`
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    /// 桶内前缀目录，可为空
    pub prefix: String,
    /// true 时用 `endpoint/bucket/key`，false 时用 `bucket.endpoint/key`
    pub path_style: bool,
}

impl S3Config {
    pub fn is_ready(&self) -> bool {
        !self.endpoint.trim().is_empty()
            && !self.bucket.trim().is_empty()
            && !self.access_key.trim().is_empty()
            && !self.secret_key.trim().is_empty()
    }

    fn region_or_default(&self) -> &str {
        let r = self.region.trim();
        if r.is_empty() {
            "us-east-1"
        } else {
            r
        }
    }
}

struct Endpoint {
    scheme: String,
    host: String,
    port: Option<u16>,
    base_path: String,
}

impl Endpoint {
    fn parse(endpoint: &str) -> Result<Self> {
        let raw = if endpoint.contains("://") {
            endpoint.to_string()
        } else {
            format!("https://{}", endpoint)
        };
        let url =
            url::Url::parse(&raw).map_err(|e| Error::Http(format!("S3 Endpoint 不合法：{}", e)))?;
        let scheme = url.scheme().to_string();
        if scheme != "http" && scheme != "https" {
            return Err(Error::Http("S3 Endpoint 只支持 http/https".into()));
        }
        let host = url
            .host_str()
            .ok_or_else(|| Error::Http("S3 Endpoint 缺少主机名".into()))?
            .to_string();
        Ok(Self {
            scheme,
            host,
            port: url.port(),
            base_path: url.path().trim_end_matches('/').to_string(),
        })
    }

    fn authority(&self, host_override: Option<&str>) -> String {
        let host = host_override.unwrap_or(&self.host);
        match self.port {
            Some(p) => format!("{}:{}", host, p),
            None => host.to_string(),
        }
    }
}

#[derive(Clone)]
pub struct S3 {
    cfg: S3Config,
    http: reqwest::Client,
}

impl S3 {
    pub fn new(cfg: S3Config, http: reqwest::Client) -> Self {
        Self { cfg, http }
    }

    pub fn config(&self) -> &S3Config {
        &self.cfg
    }

    fn endpoint(&self) -> Result<Endpoint> {
        Endpoint::parse(self.cfg.endpoint.trim())
    }

    /// 请求真正使用的 Host 头（path-style 不带 bucket，virtual 带前缀）
    fn host_header(&self) -> Result<String> {
        let ep = self.endpoint()?;
        let bucket = self.cfg.bucket.trim().trim_matches('/');
        if self.cfg.path_style {
            Ok(ep.authority(None))
        } else {
            Ok(ep.authority(Some(&format!("{}.{}", bucket, ep.host))))
        }
    }

    /// 把 `key` 拼成完整的规范路径（已做 AWS 风格 percent-encoding）
    fn object_path(&self, key: &str) -> Result<(String, String)> {
        let ep = self.endpoint()?;
        let bucket = self.cfg.bucket.trim().trim_matches('/');
        let key = key.trim_start_matches('/');
        let mut path = ep.base_path.clone();
        if self.cfg.path_style {
            path.push('/');
            path.push_str(bucket);
        }
        path.push('/');
        path.push_str(&uri_encode(key, false));
        let host = self.host_header()?;
        let url = format!("{}://{}{}", ep.scheme, host, path);
        Ok((url, path))
    }

    /// 写入一个对象。
    ///
    /// 大文件走分片上传（CreateMultipartUpload / UploadPart / CompleteMultipartUpload）：
    /// 反向代理（openresty / nginx）常把 `client_max_body_size` 卡住，整包 PUT 会被
    /// 直接挡成 413，分片后单个请求只有 16MB。
    ///
    /// 分片也不总是安全区：代理上限可能被配成 10MB、8MB。所以整包被 413 挡回来时
    /// 自动改走分片，分片被挡回来时把分片减半，最小到 5MB（S3 协议下限）。
    pub async fn put_object(&self, key: &str, body: Vec<u8>, content_type: &str) -> Result<()> {
        let (url, path) = self.object_path(key)?;
        let mut last_too_large: Option<TooLarge> = None;

        // 体积不大就先整包 PUT：一次请求，比「初始化 + 逐片 + 合并」省事
        if body.len() <= MULTIPART_THRESHOLD {
            match self
                .put_object_single(&url, &path, &body, content_type)
                .await
            {
                Ok(()) => return Ok(()),
                Err(StepError::Fatal(e)) => return Err(e),
                Err(StepError::TooLarge(t)) => last_too_large = Some(t),
            }
        }

        // 整包不行 / 文件本来就大：16MB → 8MB → 5MB，被 413 挡回来就缩一档
        let candidates = part_size_candidates(body.len());
        for part_size in &candidates {
            match self
                .put_object_multipart(&url, &path, &body, content_type, *part_size)
                .await
            {
                Ok(()) => return Ok(()),
                Err(StepError::Fatal(e)) => return Err(e),
                Err(StepError::TooLarge(t)) => last_too_large = Some(t),
            }
        }

        Err(match last_too_large {
            Some(t) => too_large_error(&self.cfg.endpoint, &t, &candidates),
            None => Error::Http(format!(
                "S3 上传失败：{} 字节的对象整包与分片都没走通",
                body.len()
            )),
        })
    }

    /// 整包 PUT（小文件：封面、歌词、收藏标记等）
    async fn put_object_single(
        &self,
        url: &str,
        path: &str,
        body: &[u8],
        content_type: &str,
    ) -> StepResult<()> {
        let payload_hash = sha256_hex(body);
        let headers = self.sign("PUT", path, "", &payload_hash, &[])?;

        let mut req = self
            .http
            .put(url)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .header(reqwest::header::CONTENT_LENGTH, body.len().to_string());
        for (k, v) in &headers {
            req = req.header(k.as_str(), v);
        }
        let resp = req
            .body(body.to_vec())
            .send()
            .await
            .map_err(|e| StepError::Fatal(Error::Http(format!("S3 上传请求失败：{}", e))))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let text = resp.text().await.unwrap_or_default();
            return Err(step_error(status, body.len(), "整包 PUT".into(), text));
        }
        Ok(())
    }

    /// 一次完整的分片上传：初始化 → 逐段 PUT → 合并；任一步失败都 abort，避免留下孤儿分片。
    ///
    /// 413 单独往回传：那是「这一片太大」，换个更小的 `part_size` 再来一遍就行。
    async fn put_object_multipart(
        &self,
        url: &str,
        path: &str,
        body: &[u8],
        content_type: &str,
        part_size: usize,
    ) -> StepResult<()> {
        let upload_id = self.initiate_multipart(url, path, content_type).await?;
        let done = match self
            .upload_parts(url, path, &upload_id, body, part_size)
            .await
        {
            Ok(parts) => self.complete_multipart(url, path, &upload_id, &parts).await,
            Err(e) => Err(e),
        };
        if done.is_err() {
            let _ = self.abort_multipart(url, path, &upload_id).await;
        }
        done
    }

    async fn initiate_multipart(
        &self,
        url: &str,
        path: &str,
        content_type: &str,
    ) -> StepResult<String> {
        let empty_hash = sha256_hex(&[]);
        // 规范查询串里空值参数写成 `uploads=`
        let headers = self.sign("POST", path, "uploads=", &empty_hash, &[])?;
        let mut req = self
            .http
            .post(format!("{}?uploads", url))
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .header(reqwest::header::CONTENT_LENGTH, "0");
        for (k, v) in &headers {
            req = req.header(k.as_str(), v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| StepError::Fatal(Error::Http(format!("S3 分片初始化失败：{}", e))))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(step_error(status, 0, "分片初始化".into(), text));
        }
        extract_tag(&text, "UploadId").ok_or_else(|| {
            StepError::Fatal(Error::Http(format!(
                "S3 分片初始化没有返回 UploadId：{}",
                text.chars().take(200).collect::<String>()
            )))
        })
    }

    async fn upload_parts(
        &self,
        url: &str,
        path: &str,
        upload_id: &str,
        body: &[u8],
        part_size: usize,
    ) -> StepResult<Vec<(u32, String)>> {
        let encoded_id = uri_encode(upload_id, true);
        let total = body.len().div_ceil(part_size.max(1));
        let mut parts = Vec::new();
        for (idx, chunk) in body.chunks(part_size).enumerate() {
            let number = (idx + 1) as u32;
            let query = format!("partNumber={}&uploadId={}", number, encoded_id);
            let payload_hash = sha256_hex(chunk);
            let headers = self.sign("PUT", path, &query, &payload_hash, &[])?;
            let mut req = self
                .http
                .put(format!("{}?{}", url, query))
                .header(reqwest::header::CONTENT_LENGTH, chunk.len().to_string());
            for (k, v) in &headers {
                req = req.header(k.as_str(), v);
            }
            let resp = req.body(chunk.to_vec()).send().await.map_err(|e| {
                StepError::Fatal(Error::Http(format!("S3 分片 {} 上传失败：{}", number, e)))
            })?;
            let status = resp.status().as_u16();
            if !(200..300).contains(&status) {
                let text = resp.text().await.unwrap_or_default();
                return Err(step_error(
                    status,
                    chunk.len(),
                    format!("分片 {}/{}", number, total),
                    text,
                ));
            }
            let etag = resp
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_string())
                .ok_or_else(|| {
                    StepError::Fatal(Error::Http(format!("S3 分片 {} 没有返回 ETag", number)))
                })?;
            parts.push((number, etag));
        }
        Ok(parts)
    }

    async fn complete_multipart(
        &self,
        url: &str,
        path: &str,
        upload_id: &str,
        parts: &[(u32, String)],
    ) -> StepResult<()> {
        let xml = complete_multipart_body(parts);
        let xml_len = xml.len();
        let payload_hash = sha256_hex(xml.as_bytes());
        let query = format!("uploadId={}", uri_encode(upload_id, true));
        let headers = self.sign("POST", path, &query, &payload_hash, &[])?;
        let mut req = self
            .http
            .post(format!("{}?{}", url, query))
            .header(reqwest::header::CONTENT_TYPE, "application/xml")
            .header(reqwest::header::CONTENT_LENGTH, xml_len.to_string());
        for (k, v) in &headers {
            req = req.header(k.as_str(), v);
        }
        let resp = req
            .body(xml)
            .send()
            .await
            .map_err(|e| StepError::Fatal(Error::Http(format!("S3 分片合并失败：{}", e))))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        // S3 允许在 200 响应里返回错误 XML
        if !(200..300).contains(&status) || text.contains("<Error>") {
            return Err(step_error(status, xml_len, "分片合并".into(), text));
        }
        Ok(())
    }

    async fn abort_multipart(&self, url: &str, path: &str, upload_id: &str) -> Result<()> {
        let empty_hash = sha256_hex(&[]);
        let query = format!("uploadId={}", uri_encode(upload_id, true));
        let headers = self.sign("DELETE", path, &query, &empty_hash, &[])?;
        let mut req = self.http.delete(format!("{}?{}", url, query));
        for (k, v) in &headers {
            req = req.header(k.as_str(), v);
        }
        let _ = req.send().await;
        Ok(())
    }

    /// 读取对象内容；对象不存在时返回 `Ok(None)`（比如库里的歌已经被删掉）
    pub async fn get_object(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let (url, path) = self.object_path(key)?;
        let empty_hash = sha256_hex(&[]);
        let headers = self.sign("GET", &path, "", &empty_hash, &[])?;

        let mut req = self.http.get(&url);
        for (k, v) in &headers {
            req = req.header(k.as_str(), v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Http(format!("S3 读取请求失败：{}", e)))?;

        let status = resp.status();
        if status.as_u16() == 404 {
            return Ok(None);
        }
        if !status.is_success() {
            let code = status.as_u16();
            let text = resp.text().await.unwrap_or_default();
            let brief: String = text.chars().take(240).collect();
            return Err(Error::Http(format!(
                "S3 读取失败（HTTP {}）：{}",
                code, brief
            )));
        }
        Ok(Some(resp.bytes().await?.to_vec()))
    }

    /// HEAD 一个对象，只取 Content-Length（不存在或失败时返回 None）。
    ///
    /// 去重维护想知道能释放多少空间，不值得把对象下载下来。
    pub async fn head_object_size(&self, key: &str) -> Result<Option<u64>> {
        let (url, path) = self.object_path(key)?;
        let empty_hash = sha256_hex(&[]);
        let headers = self.sign("HEAD", &path, "", &empty_hash, &[])?;
        let mut req = self.http.head(&url);
        for (k, v) in &headers {
            req = req.header(k.as_str(), v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Http(format!("S3 读取请求失败：{}", e)))?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        // HEAD 的 body 是空的，reqwest 的 content_length() 会给 0；直接读响应头
        Ok(resp
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok()))
    }

    /// 列出前缀下的对象 key（最多 1000 个，单页上限）
    pub async fn list_keys(&self, prefix: &str) -> Result<Vec<String>> {
        Ok(self.list_keys_page(prefix, None).await?.0)
    }

    /// 列出前缀下的**全部**对象 key：自动翻页。
    ///
    /// 完整性检测要看到整个库，单页 1000 个不够。
    pub async fn list_keys_all(&self, prefix: &str) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        // 上限只是防服务端返回异常 token 时死循环：1000 页 = 100 万对象
        for _ in 0..1_000 {
            let (keys, next) = self.list_keys_page(prefix, token.as_deref()).await?;
            out.extend(keys);
            match next {
                Some(t) => token = Some(t),
                None => break,
            }
        }
        Ok(out)
    }

    async fn list_keys_page(
        &self,
        prefix: &str,
        token: Option<&str>,
    ) -> Result<(Vec<String>, Option<String>)> {
        let (url, raw_path) = self.object_path("")?;
        let base = url.trim_end_matches('/');
        // 列表请求打的是「桶根」，路径末尾不能带 /，否则规范请求与实际请求对不上签名
        let path = {
            let p = raw_path.trim_end_matches('/');
            if p.is_empty() {
                "/".to_string()
            } else {
                p.to_string()
            }
        };
        let mut query: Vec<(&str, String)> = vec![
            ("list-type", "2".to_string()),
            ("max-keys", "1000".to_string()),
        ];
        if !prefix.is_empty() {
            query.push(("prefix", prefix.to_string()));
        }
        if let Some(t) = token {
            query.push(("continuation-token", t.to_string()));
        }
        query.sort_by(|a, b| a.0.cmp(b.0));
        // 注意：查询串里的 `/` 必须编码成 %2F。路径里的 `/` 才需要保留 ——
        // 两者用不同的规则，否则服务端按自己的规则重算签名会对不上，
        // 表现为「前缀里 case 一出现中文/斜杠就 SignatureDoesNotMatch」。
        let canonical_query = query
            .iter()
            .map(|(k, v)| format!("{}={}", uri_encode(k, true), uri_encode(v, true)))
            .collect::<Vec<_>>()
            .join("&");
        let url = format!("{}?{}", base, canonical_query);
        let empty_hash = sha256_hex(&[]);
        let headers = self.sign("GET", &path, &canonical_query, &empty_hash, &[])?;

        let mut req = self.http.get(&url);
        for (k, v) in &headers {
            req = req.header(k.as_str(), v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Http(format!("S3 列表请求失败：{}", e)))?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            let brief: String = text.chars().take(240).collect();
            return Err(Error::Http(format!(
                "S3 列表失败（HTTP {}）：{}",
                status, brief
            )));
        }
        let body = resp.text().await?;
        let truncated = extract_tag(&body, "IsTruncated")
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let next = if truncated {
            extract_tag(&body, "NextContinuationToken")
        } else {
            None
        };
        Ok((parse_keys(&body), next))
    }

    /// 连通性测试：能列出对象就算通过
    pub async fn test(&self) -> Result<String> {
        let prefix = self.normalized_prefix();
        let keys = self.list_keys(&prefix).await?;
        let bucket = self.cfg.bucket.trim();
        Ok(format!(
            "连接成功：bucket「{}」可访问，前缀「{}」下已有 {} 个对象",
            bucket,
            if prefix.is_empty() { "/" } else { &prefix },
            keys.len()
        ))
    }

    pub fn normalized_prefix(&self) -> String {
        self.cfg.prefix.trim().trim_matches('/').to_string()
    }

    /// 生成 SigV4 头：x-amz-date / x-amz-content-sha256 / authorization
    fn sign(
        &self,
        method: &str,
        canonical_uri: &str,
        canonical_query: &str,
        payload_hash: &str,
        extra: &[(&str, String)],
    ) -> Result<Vec<(String, String)>> {
        let host = self.host_header()?;
        let (amz_date, date_stamp) = utc_now_parts();
        let region = self.cfg.region_or_default();

        let mut headers: Vec<(String, String)> = vec![
            ("host".into(), host),
            ("x-amz-content-sha256".into(), payload_hash.into()),
            ("x-amz-date".into(), amz_date.clone()),
        ];
        for (k, v) in extra {
            headers.push((k.to_ascii_lowercase(), v.clone()));
        }
        headers.sort_by(|a, b| a.0.cmp(&b.0));

        let canonical_headers: String = headers
            .iter()
            .map(|(k, v)| format!("{}:{}\n", k, v.trim()))
            .collect();
        let signed_headers = headers
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(";");

        let canonical_request = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            method, canonical_uri, canonical_query, canonical_headers, signed_headers, payload_hash
        );
        let scope = format!("{}/{}/s3/aws4_request", date_stamp, region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date,
            scope,
            sha256_hex(canonical_request.as_bytes())
        );

        let k_date = hmac_sha256(
            format!("AWS4{}", self.cfg.secret_key.trim()).as_bytes(),
            date_stamp.as_bytes(),
        );
        let k_region = hmac_sha256(&k_date, region.as_bytes());
        let k_service = hmac_sha256(&k_region, b"s3");
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        let signature = hex(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.cfg.access_key.trim(),
            scope,
            signed_headers,
            signature
        );

        let mut out = vec![
            ("x-amz-date".to_string(), amz_date),
            ("x-amz-content-sha256".to_string(), payload_hash.to_string()),
            ("authorization".to_string(), authorization),
        ];
        for (k, v) in extra {
            out.push((k.to_string(), v.clone()));
        }
        Ok(out)
    }

    /// 删除对象（歌单删除/重命名时用）
    pub async fn delete_object(&self, key: &str) -> Result<()> {
        let (url, path) = self.object_path(key)?;
        let empty_hash = sha256_hex(&[]);
        let headers = self.sign("DELETE", &path, "", &empty_hash, &[])?;

        let mut req = self.http.delete(&url);
        for (k, v) in &headers {
            req = req.header(k.as_str(), v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Http(format!("S3 删除请求失败：{}", e)))?;
        if !resp.status().is_success() && resp.status().as_u16() != 204 {
            let code = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            let brief: String = text.chars().take(240).collect();
            return Err(Error::Http(format!(
                "S3 删除失败（HTTP {}）：{}",
                code, brief
            )));
        }
        Ok(())
    }

    /// 移动对象：读 → 写新 key → 删旧 key。
    ///
    /// 没有用服务端 CopyObject：实测 RustFS 对带 `x-amz-copy-source` 的请求
    /// 签名处理与 AWS 不一致（SignatureDoesNotMatch），而 GET/PUT/DELETE 都是通的。
    pub async fn move_object(&self, from_key: &str, to_key: &str) -> Result<()> {
        let data = self
            .get_object(from_key)
            .await?
            .ok_or_else(|| Error::Http("源对象不存在".into()))?;
        self.put_object(to_key, data, content_type_for_key(to_key))
            .await?;
        self.delete_object(from_key).await
    }

    /// 复制对象：读源 → 写目标（同样不走服务端 CopyObject）。
    ///
    /// 源对象不存在时返回 Ok(false)，调用方据此区分「文件已经没了」和网络错误。
    pub async fn copy_object(&self, from_key: &str, to_key: &str) -> Result<bool> {
        let Some(data) = self.get_object(from_key).await? else {
            return Ok(false);
        };
        self.put_object(to_key, data, content_type_for_key(to_key))
            .await?;
        Ok(true)
    }

    /// 生成带签名的 GET 直链（给 <audio>/<img> 直接用，默认 6 小时）
    pub fn presign_get(&self, key: &str, expires_secs: u64) -> Result<String> {
        let (url, path) = self.object_path(key)?;
        let host = self.host_header()?;
        let (amz_date, date_stamp) = utc_now_parts();
        let region = self.cfg.region_or_default();
        let scope = format!("{}/{}/s3/aws4_request", date_stamp, region);
        let expires = expires_secs.clamp(60, 604_800);

        let mut params: Vec<(&str, String)> = vec![
            ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
            (
                "X-Amz-Credential",
                format!("{}/{}", self.cfg.access_key.trim(), scope),
            ),
            ("X-Amz-Date", amz_date.clone()),
            ("X-Amz-Expires", expires.to_string()),
            ("X-Amz-SignedHeaders", "host".to_string()),
        ];
        params.sort_by(|a, b| a.0.cmp(b.0));
        let canonical_query = params
            .iter()
            .map(|(k, v)| format!("{}={}", uri_encode(k, true), uri_encode(v, true)))
            .collect::<Vec<_>>()
            .join("&");

        let canonical_headers = format!("host:{}\n", host);
        let canonical_request = format!(
            "GET\n{}\n{}\n{}\nhost\nUNSIGNED-PAYLOAD",
            path, canonical_query, canonical_headers
        );
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date,
            scope,
            sha256_hex(canonical_request.as_bytes())
        );
        let k_date = hmac_sha256(
            format!("AWS4{}", self.cfg.secret_key.trim()).as_bytes(),
            date_stamp.as_bytes(),
        );
        let k_region = hmac_sha256(&k_date, region.as_bytes());
        let k_service = hmac_sha256(&k_region, b"s3");
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        let signature = hex(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        Ok(format!(
            "{}?{}&X-Amz-Signature={}",
            url, canonical_query, signature
        ))
    }
}

fn content_type_for_key(key: &str) -> &'static str {
    let lower = key.to_ascii_lowercase();
    if lower.ends_with(".mp3") {
        "audio/mpeg"
    } else if lower.ends_with(".flac") {
        "audio/flac"
    } else if lower.ends_with(".m4a") || lower.ends_with(".mp4") {
        "audio/mp4"
    } else if lower.ends_with(".ogg") {
        "audio/ogg"
    } else if lower.ends_with(".lrc") || lower.ends_with(".txt") {
        "text/plain; charset=utf-8"
    } else if lower.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

/// CompleteMultipartUpload 的请求体
fn complete_multipart_body(parts: &[(u32, String)]) -> String {
    let mut xml =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?><CompleteMultipartUpload>");
    for (number, etag) in parts {
        xml.push_str(&format!(
            "<Part><PartNumber>{}</PartNumber><ETag>{}</ETag></Part>",
            number, etag
        ));
    }
    xml.push_str("</CompleteMultipartUpload>");
    xml
}

/// 分片大小候选：先大后小。
///
/// 反代上限未知，先用 16MB；被 413 挡回来就减半，最小到 5MB
/// （S3 协议要求除最后一片外每片都不小于 5MB）。
fn part_size_candidates(body_len: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for size in [
        MULTIPART_PART_SIZE,
        MULTIPART_PART_SIZE / 2,
        MULTIPART_MIN_PART_SIZE,
    ] {
        // 比整个文件还大的分片等于整包，试了也是白试
        if size >= MULTIPART_MIN_PART_SIZE && size < body_len && !out.contains(&size) {
            out.push(size);
        }
    }
    out
}

/// 上传单步的错误分类：413 是「这次请求太大」，缩分片还能救；其余缩了也没用。
#[derive(Debug)]
enum StepError {
    TooLarge(TooLarge),
    Fatal(Error),
}

/// 上传单步的结果：错误已经被分成「能缩分片重试」和「只能直接报」两类
type StepResult<T> = std::result::Result<T, StepError>;

impl From<Error> for StepError {
    fn from(e: Error) -> Self {
        Self::Fatal(e)
    }
}

/// 一次 413 的现场：哪个请求、多大、服务端回了什么
#[derive(Debug)]
struct TooLarge {
    what: String,
    request_bytes: usize,
    response: String,
}

fn step_error(status: u16, request_bytes: usize, what: String, body: String) -> StepError {
    if status == 413 {
        StepError::TooLarge(TooLarge {
            what,
            request_bytes,
            response: body,
        })
    } else {
        let brief: String = body.chars().take(240).collect();
        StepError::Fatal(Error::Http(format!(
            "S3 上传失败（HTTP {}）：{}",
            status, brief
        )))
    }
}

fn mb(bytes: usize) -> String {
    format!("{:.1}MB", bytes as f64 / 1024.0 / 1024.0)
}

/// 分片已经缩到底还是 413：说清楚「哪个请求、多大」，以及该改哪里。
///
/// 旧报错不管请求是整包还是某一片，永远说「本端已按 16MB 分片」，
/// 用户照着调了服务端还是失败，还得先猜是哪一步被挡的。
fn too_large_error(endpoint: &str, last: &TooLarge, tried: &[usize]) -> Error {
    let mut msg = format!(
        "S3 上传失败（HTTP 413）：{} 的请求体 {} 超过服务端上限（目标 {}）。",
        last.what,
        mb(last.request_bytes),
        endpoint.trim()
    );
    if tried.is_empty() {
        msg.push_str(&format!(
            "文件只有 {}，比分片下限 {} 还小，没法再拆。",
            mb(last.request_bytes),
            mb(MULTIPART_MIN_PART_SIZE)
        ));
    } else {
        msg.push_str(&format!(
            "已依次按 {} 分片重试仍被拒；分片最小只能到 {}（S3 协议限制）。",
            tried.iter().map(|s| mb(*s)).collect::<Vec<_>>().join(" / "),
            mb(MULTIPART_MIN_PART_SIZE)
        ));
    }
    msg.push_str(
        "请在反向代理（nginx / openresty）上把 client_max_body_size 调到 1g 以上         （至少要大于单片大小），并确认 client_body_buffer_size 不小于单次请求体。响应：",
    );
    msg.push_str(&last.response.chars().take(240).collect::<String>());
    Error::Http(msg)
}

/// AWS 风格 percent-encoding：只保留 unreserved 字符（`/` 按需保留）
fn uri_encode(input: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        let unreserved = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if unreserved || (!encode_slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex(&h.finalize())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC 接受任意长度密钥");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

/// Unix 秒 → (20231114T221320Z, 20231114)，UTC
fn utc_now_parts() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let date_stamp = format!("{:04}{:02}{:02}", y, m, d);
    let amz_date = format!("{}T{:02}{:02}{:02}Z", date_stamp, hh, mm, ss);
    (amz_date, date_stamp)
}

/// Howard Hinnant 的 days_from_civil 逆运算
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 从 ListObjectsV2 的 XML 里抠出 `<Key>`
fn parse_keys(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<Key>") {
        let after = &rest[start + 5..];
        let Some(end) = after.find("</Key>") else {
            break;
        };
        out.push(xml_unescape(&after[..end]));
        rest = &after[end + 6..];
    }
    out
}

/// 从 XML 里取一个标签的文本值（用于 IsTruncated / NextContinuationToken）
fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml_unescape(&xml[start..end]))
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_reserved_chars() {
        assert_eq!(
            uri_encode("a b/中文.mp3", false),
            "a%20b/%E4%B8%AD%E6%96%87.mp3"
        );
        assert_eq!(uri_encode("a/b", true), "a%2Fb");
        // 查询串里的斜杠必须编码，路径里必须保留
        assert_eq!(
            uri_encode("TinyMusic/测试歌单/", true),
            "TinyMusic%2F%E6%B5%8B%E8%AF%95%E6%AD%8C%E5%8D%95%2F"
        );
    }

    #[test]
    fn formats_utc_timestamp() {
        // 1700000000 = 2023-11-14 22:13:20 UTC
        let days = 1_700_000_000i64.div_euclid(86_400);
        let rem = 1_700_000_000i64.rem_euclid(86_400);
        let (y, m, d) = civil_from_days(days);
        let stamp = format!("{:04}{:02}{:02}", y, m, d);
        assert_eq!(stamp, "20231114");
        assert_eq!((rem / 3600, (rem % 3600) / 60, rem % 60), (22, 13, 20));
    }

    #[test]
    fn builds_complete_multipart_body() {
        let parts = vec![
            (1u32, "\"etag-1\"".to_string()),
            (2, "\"etag-2\"".to_string()),
        ];
        let xml = complete_multipart_body(&parts);
        assert!(xml.contains("<PartNumber>1</PartNumber><ETag>\"etag-1\"</ETag>"));
        assert!(xml.contains("<PartNumber>2</PartNumber><ETag>\"etag-2\"</ETag>"));
        assert!(xml.ends_with("</CompleteMultipartUpload>"));
    }

    #[test]
    fn explains_413_as_proxy_limit() {
        let err = too_large_error(
            "https://rustfs.example.com",
            &TooLarge {
                what: "分片 2/4".into(),
                request_bytes: MULTIPART_MIN_PART_SIZE,
                response: "<html>413</html>".into(),
            },
            &[
                MULTIPART_PART_SIZE,
                MULTIPART_PART_SIZE / 2,
                MULTIPART_MIN_PART_SIZE,
            ],
        )
        .to_string();
        assert!(err.contains("client_max_body_size"));
        assert!(err.contains("16.0MB"));
        // 哪一步被挡的要说清楚，不能再笼统说「已按 16MB 分片」
        assert!(err.contains("分片 2/4"));
        assert!(err.contains("5.0MB"));
        // 服务端返回的 HTML 也带上，方便对照是哪一层挡的
        assert!(err.contains("<html>413</html>"));
        // 打到哪个 endpoint 也要写出来：同样报 413，换个地址就是另一层代理
        assert!(err.contains("rustfs.example.com"));
    }

    #[test]
    fn tiny_body_413_says_cannot_split_further() {
        let err = too_large_error(
            "https://rustfs.example.com",
            &TooLarge {
                what: "整包 PUT".into(),
                request_bytes: 3 * 1024 * 1024,
                response: String::new(),
            },
            &[],
        )
        .to_string();
        assert!(err.contains("没法再拆"));
        assert!(err.contains("client_max_body_size"));
    }

    #[test]
    fn non_413_upload_errors_stay_plain() {
        match step_error(403, 12, "整包 PUT".into(), "AccessDenied".into()) {
            StepError::Fatal(e) => {
                let text = e.to_string();
                assert!(text.contains("HTTP 403"));
                assert!(!text.contains("client_max_body_size"));
            }
            StepError::TooLarge(_) => panic!("403 不该当成体积问题"),
        }
    }

    #[test]
    fn shrinks_part_size_when_413() {
        let mib = 1024 * 1024;
        assert_eq!(
            part_size_candidates(30 * mib),
            vec![16 * mib, 8 * mib, 5 * mib],
            "大文件先 16MB，再 8MB，最后 5MB"
        );
        assert_eq!(
            part_size_candidates(6 * mib),
            vec![5 * mib],
            "只有 5MB 一档"
        );
        assert!(
            part_size_candidates(3 * mib).is_empty(),
            "比分片下限还小的文件拆不动"
        );
    }

    #[test]
    fn parses_truncation_and_token() {
        let xml = r#"<ListBucketResult><IsTruncated>true</IsTruncated>
<NextContinuationToken>1ueGcxLPRx/abc+def=</NextContinuationToken></ListBucketResult>"#;
        assert_eq!(extract_tag(xml, "IsTruncated").as_deref(), Some("true"));
        assert_eq!(
            extract_tag(xml, "NextContinuationToken").as_deref(),
            Some("1ueGcxLPRx/abc+def=")
        );
        assert_eq!(extract_tag(xml, "Nope"), None);
    }

    #[test]
    fn parses_listing_xml() {
        let xml = r#"<ListBucketResult><Contents><Key>a &amp; b.mp3</Key></Contents>
<Contents><Key>TinyMusic/歌单/x.flac</Key></Contents></ListBucketResult>"#;
        let keys = parse_keys(xml);
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], "a & b.mp3");
        assert_eq!(keys[1], "TinyMusic/歌单/x.flac");
    }

    #[test]
    fn builds_path_style_url() {
        let cfg = S3Config {
            endpoint: "http://127.0.0.1:9000".into(),
            region: "us-east-1".into(),
            bucket: "music".into(),
            access_key: "ak".into(),
            secret_key: "sk".into(),
            prefix: "TinyMusic".into(),
            path_style: true,
        };
        let s3 = S3::new(cfg, reqwest::Client::new());
        let (url, path) = s3.object_path("TinyMusic/歌单/a b.mp3").unwrap();
        assert_eq!(
            url,
            "http://127.0.0.1:9000/music/TinyMusic/%E6%AD%8C%E5%8D%95/a%20b.mp3"
        );
        assert_eq!(path, "/music/TinyMusic/%E6%AD%8C%E5%8D%95/a%20b.mp3");
    }

    #[test]
    fn builds_virtual_host_style_url() {
        let cfg = S3Config {
            endpoint: "https://s3.us-east-1.amazonaws.com".into(),
            region: "us-east-1".into(),
            bucket: "music".into(),
            access_key: "ak".into(),
            secret_key: "sk".into(),
            prefix: String::new(),
            path_style: false,
        };
        let s3 = S3::new(cfg, reqwest::Client::new());
        let (url, _) = s3.object_path("a.mp3").unwrap();
        assert_eq!(url, "https://music.s3.us-east-1.amazonaws.com/a.mp3");
    }
}
