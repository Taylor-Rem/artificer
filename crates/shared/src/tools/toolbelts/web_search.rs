use anyhow::Result;
use scraper::{Html, Selector};
use crate::{register_toolbelt, ToolLocation};

const BRAVE_API_BASE: &str = "https://api.search.brave.com/res/v1";

pub struct WebSearch {
    api_key: String,
}

impl Default for WebSearch {
    fn default() -> Self {
        Self {
            api_key: std::env::var("BRAVE_API_KEY")
                .expect("BRAVE_API_KEY environment variable not set"),
        }
    }
}

register_toolbelt! {
    WebSearch {
        description: "Search the web and fetch webpage content using Brave Search API",
        location: ToolLocation::Server,
        tools: {
            "search" => search {
                description: "Search the web for information. Returns titles, URLs, and rich snippets. Use for general queries.",
                params: [
                    "query": "string" => "Search query",
                    "max_results": "integer" => "Maximum number of results to return (default: 5, max: 10)"
                ]
            },
            "search_news" => search_news {
                description: "Search for recent news articles on a topic. Returns articles with publish dates and descriptions. Use for news, current events, or anything time-sensitive.",
                params: [
                    "query": "string" => "News search query",
                    "max_results": "integer" => "Maximum number of results to return (default: 5, max: 10)"
                ]
            },
            "fetch_page" => fetch_page {
                description: "Fetch a webpage and extract readable text content. Use after search to read full articles.",
                params: [
                    "url": "string" => "URL to fetch"
                ]
            }
        }
    }
}

impl WebSearch {
    fn search(&self, args: &serde_json::Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        if query.is_empty() {
            return Ok("Error: query cannot be empty".to_string());
        }

        let max_results = args["max_results"]
            .as_i64()
            .unwrap_or(5)
            .min(10) as usize;

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.search_web(query, max_results).await
            })
        })
    }

    fn search_news(&self, args: &serde_json::Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        if query.is_empty() {
            return Ok("Error: query cannot be empty".to_string());
        }

        let max_results = args["max_results"]
            .as_i64()
            .unwrap_or(5)
            .min(10) as usize;

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.search_news_async(query, max_results).await
            })
        })
    }

    fn fetch_page(&self, args: &serde_json::Value) -> Result<String> {
        let url = args["url"].as_str().unwrap_or("");
        if url.is_empty() {
            return Ok("Error: url cannot be empty".to_string());
        }

        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok("Error: URL must start with http:// or https://".to_string());
        }

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                self.fetch_page_async(url).await
            })
        })
    }

    fn brave_client(&self) -> Result<reqwest::Client> {
        Ok(reqwest::Client::builder()
            .user_agent("Artificer/0.1")
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    "Accept",
                    "application/json".parse().unwrap(),
                );
                // headers.insert(
                //     "Accept-Encoding",
                //     "gzip".parse().unwrap(),
                // );
                headers.insert(
                    "X-Subscription-Token",
                    self.api_key.parse().unwrap(),
                );
                headers
            })
            .build()?)
    }

    async fn search_web(&self, query: &str, max_results: usize) -> Result<String> {
        let client = self.brave_client()?;
        let count_str = max_results.to_string();

        let response = client
            .get(format!("{}/web/search", BRAVE_API_BASE))
            .query(&[
                ("q", query),
                ("count", count_str.as_str()),
                ("text_decorations", "false"),
                ("search_lang", "en"),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(format!("Search failed: {}", response.status()));
        }

        let data: serde_json::Value = response.json().await?;

        let mut output = String::new();
        output.push_str(&format!("Search results for '{}':\n\n", query));

        if let Some(results) = data["web"]["results"].as_array() {
            for (i, result) in results.iter().take(max_results).enumerate() {
                let title = result["title"].as_str().unwrap_or("No title");
                let url = result["url"].as_str().unwrap_or("");
                let description = result["description"].as_str().unwrap_or("");

                output.push_str(&format!("{}. {}\n", i + 1, title));
                output.push_str(&format!("   URL: {}\n", url));
                if !description.is_empty() {
                    output.push_str(&format!("   {}\n", description));
                }
                output.push('\n');
            }
        } else {
            output.push_str("No results found.\n");
        }

        Ok(output)
    }

    async fn search_news_async(&self, query: &str, max_results: usize) -> Result<String> {
        let client = self.brave_client()?;
        let count_str = max_results.to_string();

        let response = client
            .get(format!("{}/news/search", BRAVE_API_BASE))
            .query(&[
                ("q", query),
                ("count", count_str.as_str()),
                ("text_decorations", "false"),
                ("search_lang", "en"),
                ("freshness", "pd"), // past day
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(format!("News search failed: {}", response.status()));
        }

        let data: serde_json::Value = response.json().await?;

        eprintln!("News API response structure: {}",
                  serde_json::to_string_pretty(&data).unwrap_or_default());

        let mut output = String::new();
        output.push_str(&format!("News results for '{}':\n\n", query));

        if let Some(results) = data["results"].as_array() {
            if results.is_empty() {
                output.push_str("No recent news found. Try a broader query or use regular search.\n");
                return Ok(output);
            }

            for (i, article) in results.iter().take(max_results).enumerate() {
                let title = article["title"].as_str().unwrap_or("No title");
                let url = article["url"].as_str().unwrap_or("");
                let description = article["description"].as_str().unwrap_or("");
                let source = article["meta_url"]["hostname"].as_str().unwrap_or("");
                let age = article["age"].as_str().unwrap_or("");

                output.push_str(&format!("{}. {}\n", i + 1, title));
                if !source.is_empty() {
                    output.push_str(&format!("   Source: {}", source));
                    if !age.is_empty() {
                        output.push_str(&format!(" — {}", age));
                    }
                    output.push('\n');
                }
                output.push_str(&format!("   URL: {}\n", url));
                if !description.is_empty() {
                    output.push_str(&format!("   {}\n", description));
                }
                output.push('\n');
            }
        } else {
            output.push_str("No results found.\n");
        }

        Ok(output)
    }

    async fn fetch_page_async(&self, url: &str) -> Result<String> {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:120.0) Gecko/20100101 Firefox/120.0")
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        let response = client.get(url).send().await?;

        if !response.status().is_success() {
            return Ok(format!("Failed to fetch page: {}", response.status()));
        }

        // Check content type — skip binary files
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !content_type.contains("text/html") && !content_type.contains("text/plain") {
            return Ok(format!("Skipping non-text content type: {}", content_type));
        }

        let html = response.text().await?;
        let text = self.html_to_text(&html)?;

        // Trim aggressively — the model doesn't need 50k chars of boilerplate
        let trimmed = if text.len() > 20_000 {
            format!("{}... [truncated at 20,000 chars, full page is {} chars]",
                    &text[..20_000],
                    text.len())
        } else {
            text
        };

        Ok(trimmed)
    }

    fn html_to_text(&self, html: &str) -> Result<String> {
        let document = Html::parse_document(html);

        // Remove noisy elements before extracting text
        let body_selector = Selector::parse("body").unwrap();
        let mut text_parts = Vec::new();

        if let Some(body) = document.select(&body_selector).next() {
            extract_text(&body, &mut text_parts);
        }

        let lines: Vec<&str> = text_parts
            .iter()
            .map(|s| s.trim())
            .filter(|s| {
                !s.is_empty()
                    // Filter out very short noise lines
                    && s.len() > 3
                    // Filter out lines that are just symbols/numbers
                    && s.chars().any(|c| c.is_alphabetic())
            })
            .collect();

        Ok(lines.join("\n"))
    }
}

fn extract_text(element: &scraper::ElementRef, parts: &mut Vec<String>) {
    let tag = element.value().name();

    // Skip entirely
    if matches!(tag, "script" | "style" | "nav" | "footer" | "header" | "aside" | "noscript") {
        return;
    }

    for child in element.children() {
        if let Some(text) = child.value().as_text() {
            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                parts.push(trimmed);
            }
        } else if let Some(child_ref) = child.value().as_element().and_then(|_| scraper::ElementRef::wrap(child)) {
            extract_text(&child_ref, parts);
        }
    }
}
