use crate::Message;

async fn call_model_simple(
    gpu: &GpuHandle,
    messages: Vec<Message>,
    client: &reqwest::Client,
) -> Result<Message> {
    // TODO: Implement simple model call for background agents
    todo!("call_model_simple not yet implemented")
}