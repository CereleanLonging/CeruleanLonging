//! Provider wire formats. Continuation data never enters a serializable reply.
use super::*;
use serde_json::json;

fn anthropic(r: &ApiRequest) -> bool {
    r.target.provider() == CredentialProvider::Anthropic
}
fn output(result: &ToolResult) -> String {
    json!({"isError":result.is_error,"content":result.text}).to_string()
}
pub(super) fn body(r: &ApiRequest) -> Result<Value, String> {
    let mut messages = Vec::new();
    let openai = matches!(r.target, ApiTarget::OpenAi {});
    for message in &r.messages {
        match message {
            ApiMessage::User(s) => messages.push(if openai {
                json!({"role":"user","content":[{"type":"input_text","text":s}]})
            } else if anthropic(r) {
                json!({"role":"user","content":[{"type":"text","text":s}]})
            } else {
                json!({"role":"user","content":s})
            }),
            ApiMessage::Assistant(reply) => {
                if anthropic(r) {
                    messages.push(json!({"role":"assistant","content":reply.continuation}));
                } else {
                    messages.extend(reply.continuation.iter().cloned());
                }
            }
            ApiMessage::Tools(results) => {
                if anthropic(r) {
                    let blocks:Vec<_> = results.iter().map(|t| json!({"type":"tool_result","tool_use_id":t.call_id,"content":t.text,"is_error":t.is_error})).collect();
                    messages.push(json!({"role":"user","content":blocks}));
                } else {
                    for result in results {
                        messages.push(if openai {
                            json!({"type":"function_call_output","call_id":result.call_id,"output":output(result)})
                        } else { json!({"role":"tool","tool_call_id":result.call_id,"content":output(result)}) });
                    }
                }
            }
        }
    }
    let mut body: Value = if openai {
        let tools:Vec<_> = r.tools.iter().map(|t| json!({"type":"function","name":t.name,"description":t.description,"parameters":t.parameters,"strict":false})).collect();
        json!({"model":r.model,"instructions":r.instructions,"input":messages,"tools":tools,"tool_choice":if tools.is_empty(){"none"}else{"auto"},"parallel_tool_calls":false,"max_output_tokens":r.max_output_tokens,"stream":false,"store":false,"include":["reasoning.encrypted_content"]})
    } else if anthropic(r) {
        // All results for a tool turn must be adjacent in one user message.
        let mut merged: Vec<Value> = Vec::new();
        for message in messages {
            if let Some(previous) = merged
                .last_mut()
                .filter(|p| p["role"] == "user" && message["role"] == "user")
            {
                previous["content"]
                    .as_array_mut()
                    .ok_or("invalid API content")?
                    .extend(
                        message["content"]
                            .as_array()
                            .ok_or("invalid API content")?
                            .iter()
                            .cloned(),
                    );
            } else {
                merged.push(message);
            }
        }
        let mut value = json!({"model":r.model,"system":r.instructions,"messages":merged,"max_tokens":r.max_output_tokens,"stream":false});
        if !r.tools.is_empty() {
            value["tools"]=json!(r.tools.iter().map(|t|json!({"name":t.name,"description":t.description,"input_schema":t.parameters})).collect::<Vec<_>>());
            value["tool_choice"] = json!({"type":"auto","disable_parallel_tool_use":true});
        }
        value
    } else {
        messages.insert(0, json!({"role":"system","content":r.instructions}));
        let mut value = json!({"model":r.model,"messages":messages,"max_tokens":r.max_output_tokens,"stream":false});
        if !r.tools.is_empty() {
            value["tools"]=json!(r.tools.iter().map(|t| json!({"type":"function","function":{"name":t.name,"description":t.description,"parameters":t.parameters}})).collect::<Vec<_>>());
            value["tool_choice"] = json!("auto");
        }
        value
    };
    cerulean_domain::reasoning::apply_api(&mut body, &r.target, &r.model, r.reasoning_effort)?;
    Ok(body)
}
fn string(v: &Value) -> Result<&str, String> {
    v.as_str()
        .ok_or_else(|| "API response string missing".into())
}
fn number(v: &Value) -> Result<u64, String> {
    v.as_u64()
        .ok_or_else(|| "API usage missing or invalid".into())
}
fn optional_number(v: &Value) -> Result<Option<u64>, String> {
    if v.is_null() {
        Ok(None)
    } else {
        number(v).map(Some)
    }
}
fn array(v: &Value) -> Result<&Vec<Value>, String> {
    let a = v.as_array().ok_or("API response array missing")?;
    require(a.len() <= 128, "API response has too many items")?;
    Ok(a)
}
fn append(target: &mut String, v: &Value) -> Result<(), String> {
    let s = string(v)?;
    require(
        target.len() + s.len() <= MAX_TEXT,
        "API response text too large",
    )?;
    text(s, MAX_TEXT, true)?;
    target.push_str(s);
    Ok(())
}
fn call(r: &ApiRequest, id: &Value, name: &Value, args: Value) -> Result<ToolCall, String> {
    let name = string(name)?.to_owned();
    require(
        r.tools.iter().any(|t| t.name == name),
        "API requested an unregistered tool",
    )?;
    let result = ToolCall {
        id: string(id)?.into(),
        name,
        arguments: args,
    };
    require(
        result.arguments.is_object(),
        "API tool arguments must be an object",
    )?;
    Ok(result)
}
fn arguments(v: &Value) -> Result<Value, String> {
    let raw = string(v)?;
    text(raw, MAX_TEXT, false)?;
    serde_json::from_str(raw).map_err(|_| "API tool arguments are incomplete or invalid".into())
}
fn complete(v: &Value) -> Result<(), String> {
    require(
        v.is_null() || v == "completed",
        "API output item incomplete",
    )
}

pub(super) fn parse(r: &ApiRequest, v: Value) -> Result<ApiReply, String> {
    require(
        v.is_object() && v["error"].is_null(),
        "API response contains an error",
    )?;
    let mut observation = ApiObservation {
        response_id: string(&v["id"])?.into(),
        actual_model: string(&v["model"])?.into(),
        text: String::new(),
        tool_calls: Vec::new(),
        usage: ApiUsage {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: None,
            cache_write_tokens: None,
        },
    };
    let mut continuation = Vec::new();
    if matches!(r.target, ApiTarget::OpenAi {}) {
        require(
            v["status"] == "completed" && v["incomplete_details"].is_null(),
            "API response incomplete",
        )?;
        observation.usage.input_tokens = number(&v["usage"]["input_tokens"])?;
        observation.usage.output_tokens = number(&v["usage"]["output_tokens"])?;
        observation.usage.cache_read_tokens =
            optional_number(&v["usage"]["input_tokens_details"]["cached_tokens"])?;
        observation.usage.cache_write_tokens =
            optional_number(&v["usage"]["input_tokens_details"]["cache_write_tokens"])?;
        for item in array(&v["output"])? {
            complete(&item["status"])?;
            match string(&item["type"])? {
                "message" => {
                    require(item["role"] == "assistant", "API output role invalid")?;
                    token(string(&item["id"])?)?;
                    let mut content = Vec::new();
                    for block in array(&item["content"])? {
                        require(
                            block["type"] == "output_text",
                            "API output refused or unsupported",
                        )?;
                        append(&mut observation.text, &block["text"])?;
                        content.push(
                            json!({"type":"output_text","text":block["text"],"annotations":[]}),
                        );
                    }
                    continuation.push(json!({"type":"message","id":item["id"],"role":"assistant","content":content,"status":"completed"}));
                }
                "function_call" => {
                    observation.tool_calls.push(call(
                        r,
                        &item["call_id"],
                        &item["name"],
                        arguments(&item["arguments"])?,
                    )?);
                    continuation.push(json!({"type":"function_call","call_id":item["call_id"],"name":item["name"],"arguments":item["arguments"]}));
                }
                "reasoning" => {
                    text(string(&item["encrypted_content"])?, MAX_RESPONSE, false)?;
                    let mut value = json!({"type":"reasoning","encrypted_content":item["encrypted_content"],"summary":[]});
                    if !item["id"].is_null() {
                        token(string(&item["id"])?)?;
                        value["id"] = item["id"].clone();
                    }
                    continuation.push(value);
                }
                _ => return Err("API output type unsupported".into()),
            }
        }
    } else if anthropic(r) {
        require(
            v["type"] == "message" && v["role"] == "assistant",
            "API message type invalid",
        )?;
        observation.usage.input_tokens = number(&v["usage"]["input_tokens"])?;
        observation.usage.output_tokens = number(&v["usage"]["output_tokens"])?;
        observation.usage.cache_read_tokens =
            optional_number(&v["usage"]["cache_read_input_tokens"])?;
        observation.usage.cache_write_tokens =
            optional_number(&v["usage"]["cache_creation_input_tokens"])?;
        for block in array(&v["content"])? {
            continuation.push(match string(&block["type"])? {
                "text"=>{append(&mut observation.text,&block["text"])?;json!({"type":"text","text":block["text"]})}
                "tool_use"=>{observation.tool_calls.push(call(r,&block["id"],&block["name"],block["input"].clone())?);json!({"type":"tool_use","id":block["id"],"name":block["name"],"input":block["input"]})}
                "thinking"=>{text(string(&block["thinking"])?,MAX_RESPONSE,true)?;text(string(&block["signature"])?,MAX_RESPONSE,false)?;json!({"type":"thinking","thinking":block["thinking"],"signature":block["signature"]})}
                "redacted_thinking"=>{text(string(&block["data"])?,MAX_RESPONSE,false)?;json!({"type":"redacted_thinking","data":block["data"]})}
                _=>return Err("API content refused or unsupported".into()),
            });
        }
        require(
            if observation.tool_calls.is_empty() {
                v["stop_reason"] == "end_turn"
            } else {
                v["stop_reason"] == "tool_use"
            },
            "API stop reason incomplete or inconsistent",
        )?;
    } else {
        let choices = array(&v["choices"])?;
        require(choices.len() == 1, "API must return one choice")?;
        let choice = &choices[0];
        require(
            choice["finish_reason"] != "length",
            "API output token limit reached",
        )?;
        let message = &choice["message"];
        require(
            choice["index"] == 0
                && message["role"] == "assistant"
                && message["refusal"].is_null()
                && message["function_call"].is_null(),
            "API choice refused or unsupported",
        )?;
        if !message["content"].is_null() {
            append(&mut observation.text, &message["content"])?;
        }
        let mut assistant = json!({"role":"assistant","content":message["content"]});
        if !message["reasoning_content"].is_null() {
            text(string(&message["reasoning_content"])?, MAX_RESPONSE, true)?;
            assistant["reasoning_content"] = message["reasoning_content"].clone();
        }
        if !message["tool_calls"].is_null() {
            let mut calls = Vec::new();
            for item in array(&message["tool_calls"])? {
                require(item["type"] == "function", "API tool type unsupported")?;
                observation.tool_calls.push(call(
                    r,
                    &item["id"],
                    &item["function"]["name"],
                    arguments(&item["function"]["arguments"])?,
                )?);
                calls.push(json!({"id":item["id"],"type":"function","function":{"name":item["function"]["name"],"arguments":item["function"]["arguments"]}}));
            }
            assistant["tool_calls"] = json!(calls);
        }
        require(
            if observation.tool_calls.is_empty() {
                choice["finish_reason"] == "stop"
            } else {
                choice["finish_reason"] == "tool_calls"
            },
            "API finish reason incomplete or inconsistent",
        )?;
        continuation.push(assistant);
        observation.usage.input_tokens = number(&v["usage"]["prompt_tokens"])?;
        observation.usage.output_tokens = number(&v["usage"]["completion_tokens"])?;
        observation.usage.cache_read_tokens =
            optional_number(&v["usage"]["prompt_cache_hit_tokens"])?.or(optional_number(
                &v["usage"]["prompt_tokens_details"]["cached_tokens"],
            )?);
    }
    observation.validate()?;
    observation.usage.total_tokens(r.target.provider())?;
    Ok(ApiReply {
        observation,
        target: r.target.clone(),
        requested_model: r.model.clone(),
        continuation,
    })
}
