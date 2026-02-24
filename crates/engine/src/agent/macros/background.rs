#[macro_export]
macro_rules! define_background_agent {
    (
        $struct_name:ident {
            name: $name:literal,
            instructions: $instructions:literal,
        }
    ) => {
        pub struct $struct_name;

        impl $crate::agent::BackgroundAgent for $struct_name {
            fn name(&self) -> &str {
                $name
            }

            fn system_prompt(&self) -> String {
                $instructions.to_string()
            }

            async fn execute(
                &self,
                input: String,
                gpu: &$crate::pool::GpuHandle,
                client: &reqwest::Client,
            ) -> anyhow::Result<String> {
                use $crate::Message;

                let messages = vec![
                    Message {
                        role: "system".to_string(),
                        content: Some(self.system_prompt()),
                        tool_calls: None,
                    },
                    Message {
                        role: "user".to_string(),
                        content: Some(input),
                        tool_calls: None,
                    },
                ];

                // TODO: Implement call_model helper
                // For now, placeholder
                let response = call_model_simple(gpu, messages, client).await?;
                Ok(response.content.unwrap_or_default())
            }
        }
    };
}