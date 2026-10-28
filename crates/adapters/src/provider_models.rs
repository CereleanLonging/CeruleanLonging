//! Fixed-destination, bounded, non-generating model discovery. No retry or redirect.
use cerulean_domain::{credentials::ProviderEndpoint, execution::require, secret::SecretInput};
use serde_json::Value;
use std::{collections::BTreeSet, time::Duration};
const RESPONSE_LIMIT: usize = 2 * 1024 * 1024;
const MODEL_LIMIT: usize = 200;
#[derive(Debug)]
pub struct ModelList {
    pub models: Vec<String>,
    pub limited: bool,
}

pub fn endpoint(target: &ProviderEndpoint) -> Result<String, String> {
    Ok(match target {
        ProviderEndpoint::DeepSeek {} => "https://api.deepseek.com/models".into(),
        ProviderEndpoint::Anthropic {} => "https://api.anthropic.com/v1/models?limit=200".into(),
        ProviderEndpoint::KimiChina {} => "https://api.moonshot.cn/v1/models".into(),
        ProviderEndpoint::A6Api {} => "https://api.a6api.com/v1/models".into(),
        ProviderEndpoint::Grok {} => "https://api.x.ai/v1/models".into(),
        ProviderEndpoint::QwenBeijing { workspace_id } => {
            require(!workspace_id.is_empty() && workspace_id.len() <= 63 && workspace_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') && !workspace_id.starts_with('-') && !workspace_id.ends_with('-'), "请输入有效的百炼北京业务空间 ID（字母、数字及中划线）")?;
            format!("https://{}.cn-beijing.maas.aliyuncs.com/api/v1/models?page_no=1&page_size=100&providers=qwen", workspace_id.to_ascii_lowercase())
        },
        ProviderEndpoint::QwenSingapore {} => "https://dashscope-intl.aliyuncs.com/api/v1/models?page_no=1&page_size=100&providers=qwen".into(),
        ProviderEndpoint::QwenHongKong {} => "https://cn-hongkong.dashscope.aliyuncs.com/api/v1/models?page_no=1&page_size=100&providers=qwen".into(),
    })
}
fn qwen(target: &ProviderEndpoint) -> bool {
    target.provider() == cerulean_domain::credentials::CredentialProvider::Qwen
}
fn parse(target: &ProviderEndpoint, value: Value, key: &SecretInput) -> Result<ModelList, String> {
    require(
        value.get("error").is_none_or(Value::is_null),
        "供应商返回错误结构",
    )?;
    let (items, field, mut limited) = if qwen(target) {
        require(
            value["success"] == true && value["code"].is_null() && value["output"]["page_no"] == 1,
            "百炼模型列表未成功或页码不符",
        )?;
        let items = value["output"]["models"]
            .as_array()
            .ok_or("百炼模型列表缺失")?;
        let total = value["output"]["total"]
            .as_u64()
            .ok_or("百炼模型总数缺失")?;
        require(total >= items.len() as u64, "百炼模型总数与列表不符")?;
        (items, "model", total > items.len() as u64)
    } else {
        let items = value["data"].as_array().ok_or("供应商模型列表缺失")?;
        let more = if matches!(target, ProviderEndpoint::Anthropic {}) {
            value["has_more"].as_bool().ok_or("Claude 分页信息缺失")?
        } else {
            match value.get("has_more") {
                None => false,
                Some(v) => v.as_bool().ok_or("供应商分页信息无效")?,
            }
        };
        (items, "id", more)
    };
    require(items.len() <= 10000, "模型列表条目过多")?;
    let mut models = BTreeSet::new();
    for item in items {
        let id = item[field].as_str().ok_or("模型标识缺失")?;
        require(
            !id.is_empty()
                && id.len() <= 200
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
                && !id.contains(key.expose()),
            "模型标识无效",
        )?;
        if models.len() < MODEL_LIMIT {
            models.insert(id.to_owned());
        } else if !models.contains(id) {
            limited = true;
        }
    }
    Ok(ModelList {
        models: models.into_iter().collect(),
        limited,
    })
}
fn http(https_only: bool) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .https_only(https_only)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|_| "供应商 TLS 客户端不可用".into())
}
pub async fn models(target: &ProviderEndpoint, key: SecretInput) -> Result<ModelList, String> {
    let url = endpoint(target)?;
    key.validate()?;
    fetch(&http(true)?, target, &key, &url).await
}
// Private seam, not configurable through environment/IPC/production constructor.
async fn fetch(
    client: &reqwest::Client,
    target: &ProviderEndpoint,
    key: &SecretInput,
    url: &str,
) -> Result<ModelList, String> {
    let mut request = client.get(url).header("Accept", "application/json");
    if matches!(target, ProviderEndpoint::Anthropic {}) {
        let mut header =
            reqwest::header::HeaderValue::from_str(key.expose()).map_err(|_| "无效凭证格式")?;
        header.set_sensitive(true);
        request = request
            .header("x-api-key", header)
            .header("anthropic-version", "2023-06-01");
    } else {
        request = request.bearer_auth(key.expose());
    }
    let mut response = request
        .send()
        .await
        .map_err(|_| "连接失败或超时；未重试，请核对网络与地域")?;
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            401 => "认证失败；请核对 Key 和地域",
            403 => "访问被拒绝；请核对账户权限和地域",
            429 => "限流或配额不足；未重试",
            300..=399 => "供应商要求重定向；已拒绝转发凭证",
            _ => "供应商返回失败状态；未重试",
        }
        .into());
    }
    require(response.url().as_str() == url, "供应商端点发生变化")?;
    require(
        response
            .content_length()
            .is_none_or(|n| n <= RESPONSE_LIMIT as u64),
        "模型列表响应过大",
    )?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "响应中断；未重试")? {
        require(
            bytes.len() + chunk.len() <= RESPONSE_LIMIT,
            "模型列表响应过大",
        )?;
        bytes.extend_from_slice(&chunk);
    }
    let value = serde_json::from_slice(&bytes).map_err(|_| "模型列表不是有效 JSON")?;
    parse(target, value, key)
}

#[cfg(test)]
mod tests;
