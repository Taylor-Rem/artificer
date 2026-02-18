// crates/shared/src/toolbelts/web_search.rs

use anyhow::Result;
use scraper::{Html, Selector};
use crate::{register_toolbelt, ToolLocation};

pub struct WebSearch;

impl Default for WebSearch {
    fn default() -> Self {
        Self
    }
}

register_toolbelt! {
    WebSearch {
        description: "Search the web and fetch webpage content",
        location: ToolLocation::Server,
        tools: {
            "search" => search {
                description: "Search DuckDuckGo for information. Returns titles, URLs, and snippets.",
                params: [
                    "query": "string" => "Search query",
                    "max_results": "integer" => "Maximum number of results to return (default: 5, max: 10)"
                ]
            },
            "fetch_page" => fetch_page {
                description: "Fetch a webpage and convert to readable text. Use after search to read full articles.",
                params: ["url": "string" => "URL to fetch"]
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

        // Use tokio::task::block_in_place to safely run async code
        let runtime = tokio::runtime::Handle::current();
        let result = runtime.block_on(async {
            self.search_async(query, max_results).await
        })?;

        Ok(result)
    }

    fn fetch_page(&self, args: &serde_json::Value) -> Result<String> {
        let url = args["url"].as_str().unwrap_or("");
        if url.is_empty() {
            return Ok("Error: url cannot be empty".to_string());
        }

        // Validate URL
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok("Error: URL must start with http:// or https://".to_string());
        }

        let runtime = tokio::runtime::Handle::current();
        let result = runtime.block_on(async {
            self.fetch_page_async(url).await
        })?;

        Ok(result)
    }

    async fn search_async(&self, query: &str, max_results: usize) -> Result<String> {
        // Build DuckDuckGo search URL
        let encoded_query = urlencoding::encode(query);
        let search_url = format!("https://html.duckduckgo.com/html/?q={}", encoded_query);

        // Fetch search results (async)
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:120.0) Gecko/20100101 Firefox/120.0")
            .build()?;

        let response = client
            .get(&search_url)
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(format!("Search failed with status: {}", response.status()));
        }

        let html_content = response.text().await?;
        let results = self.parse_duckduckgo_results(&html_content, max_results)?;

        if results.is_empty() {
            return Ok("No results found".to_string());
        }

        // Format results as readable text
        let mut output = String::new();
        output.push_str(&format!("Found {} results for '{}':\n\n", results.len(), query));

        for (i, result) in results.iter().enumerate() {
            output.push_str(&format!("{}. {}\n", i + 1, result.title));
            output.push_str(&format!("   URL: {}\n", result.url));
            if !result.snippet.is_empty() {
                output.push_str(&format!("   {}\n", result.snippet));
            }
            output.push('\n');
        }

        Ok(output)
    }

    async fn fetch_page_async(&self, url: &str) -> Result<String> {
        // Fetch page (async)
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:120.0) Gecko/20100101 Firefox/120.0")
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        let response = client
            .get(url)
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(format!("Failed to fetch page: {}", response.status()));
        }

        let html_content = response.text().await?;

        // Convert HTML to readable text
        let readable_text = self.html_to_text(&html_content)?;

        // Limit size
        if readable_text.len() > 50_000 {
            Ok(format!("{}... [truncated, full page is {} chars]",
                       &readable_text[..50_000],
                       readable_text.len()))
        } else {
            Ok(readable_text)
        }
    }

    fn parse_duckduckgo_results(&self, html: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        let document = Html::parse_document(html);

        // DuckDuckGo HTML structure selectors
        let result_selector = Selector::parse(".result").unwrap();
        let title_selector = Selector::parse(".result__a").unwrap();
        let snippet_selector = Selector::parse(".result__snippet").unwrap();
        let url_selector = Selector::parse(".result__url").unwrap();

        let mut results = Vec::new();

        for element in document.select(&result_selector).take(max_results) {
            let title = element
                .select(&title_selector)
                .next()
                .map(|el| el.text().collect::<String>())
                .unwrap_or_default()
                .trim()
                .to_string();

            let snippet = element
                .select(&snippet_selector)
                .next()
                .map(|el| el.text().collect::<String>())
                .unwrap_or_default()
                .trim()
                .to_string();

            let url = element
                .select(&url_selector)
                .next()
                .and_then(|el| el.value().attr("href"))
                .map(|href| {
                    // DuckDuckGo wraps URLs in their redirect
                    if href.starts_with("//duckduckgo.com/l/?uddg=") {
                        urlencoding::decode(href.split("uddg=").nth(1).unwrap_or(href))
                            .unwrap_or_default()
                            .to_string()
                    } else {
                        href.to_string()
                    }
                })
                .unwrap_or_default();

            if !title.is_empty() && !url.is_empty() {
                results.push(SearchResult { title, url, snippet });
            }
        }

        Ok(results)
    }

    fn html_to_text(&self, html: &str) -> Result<String> {
        let document = Html::parse_document(html);

        let body_selector = Selector::parse("body").unwrap();
        let mut text_parts = Vec::new();

        if let Some(body) = document.select(&body_selector).next() {
            fn extract_text(element: &scraper::ElementRef, parts: &mut Vec<String>) {
                if element.value().name() == "script" || element.value().name() == "style" {
                    return;
                }

                for child in element.children() {
                    if let Some(text) = child.value().as_text() {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            parts.push(trimmed.to_string());
                        }
                    } else if let Some(_element) = child.value().as_element() {
                        if let Some(child_ref) = scraper::ElementRef::wrap(child) {
                            extract_text(&child_ref, parts);
                        }
                    }
                }
            }

            extract_text(&body, &mut text_parts);
        }

        let text = text_parts.join("\n");
        let lines: Vec<&str> = text
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect();

        Ok(lines.join("\n"))
    }
}

#[derive(Debug)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}