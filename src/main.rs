use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

mod traits;
mod toolbelts;
mod registry;
mod artificer;

use artificer::Artificer;
use crate::traits::Agent;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>
}
#[tokio::main]
async fn main() -> Result<()> {
    let artificer = Artificer;
    let mut messages = vec![Message {
        role: "system".to_string(),
        content: Some(artificer.system_prompt().to_string()),
    }];
    println!("Artificer is ready. Type 'quit' to exit.\n");
    loop {
        let input = wait_for_user_input()?;
        if input.eq_ignore_ascii_case("quit") {
            println!("Goodbye!");
            break;
        }
        if input.is_empty() {
            continue;
        }
        messages.push(Message {
            role: "user".to_string(),
            content: Some(input)
        });
        let response = artificer.make_request(&messages).await?;
        let content = response.content.clone().unwrap_or_default();
        println!("\nArtificer: {}\n", content);
        messages.push(response);
    }
    Ok(())
}

fn wait_for_user_input() -> Result<String> {
    print!("You: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}