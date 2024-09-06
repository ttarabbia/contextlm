use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

use dotenv::dotenv;
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    // let response = call_gemini(&"tell me about Oauth2".to_string(), &"Oauth2".to_string()).await?;
    //
    // for text in &response {
    //     println!("Extracted: \n {} \n", text);
    // }
    let input_file_path = "scraped_urls.txt";

    let first_page = "https://texreg.sos.state.tx.us/public/readtac$ext.TacPage?sl=T&app=9&p_dir=N&p_rloc=199238&p_tloc=&p_ploc=1&pg=2&p_tac=&ti=30&pt=1&ch=1&rl=1";
    let start_url = get_start_url("scraped_urls.txt", &first_page).await;
    let mut current_url = start_url;
    let mut all_urls = Vec::new();
    let max_pages = 1000; // Set this to your desired limit
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(input_file_path)?;

    for _ in 0..max_pages {
        let graphic_urls = add_attached_graphics_urls(&current_url, &mut all_urls).await?;

        for url in graphic_urls {
            writeln!(file, "{}", url)?;
        }

        all_urls.push(current_url.clone());
        println!("Processing: {}", current_url);

        writeln!(file, "{}", current_url)?;
        match extract_next_url(&current_url).await? {
            Some(next_url) => {
                current_url = next_url;
            }
            None => break,
        }
    }
    println!("Total URLs processed: {}", all_urls.len());
    println!("all Urls: {:?}", all_urls);

    let deduped_file = dedupe_file(&input_file_path).await?;

    let input_file = File::open(deduped_file)?;
    let reader = BufReader::new(input_file);


    let lines = reader.lines().collect::<Result<Vec<String>, io::Error>>()?;

    let max_concurrent_tasks = 100;

    // let results = 






    Ok(())
}

async fn html_to_markdown(url: &str) ->  Result<(String, String), Box<dyn std::error::Error>>{

    let resp = reqwest::get(url).await?.text().await?;
    let document = Html::parse_document(&resp);

    let content_selector = Selector::parse("body").unwrap();


    let remove_selectors = vec![
        Selector::parse("script").unwrap(),
        Selector::parse("style").unwrap(),
        Selector::parse("nav").unwrap(),
        Selector::parse("header").unwrap(),
        Selector::parse("footer").unwrap(),
        Selector::parse("center").unwrap()
    ];


    let mut cleaned_html = document.root_element().html();

    for selector in remove_selectors {
        document.select(&selector).for_each(|element| {
            let html_to_remove = element.html();
            cleaned_html = cleaned_html.replace(&html_to_remove, "");
        });
    }

    let cleaned_document = Html::parse_document(&cleaned_html);

    let main_content = cleaned_document.select(&content_selector).next().ok_or("Could not find main content")?;

    let markdown = html2md::parse_html(&main_content.inner_html());


      let table_selector = Selector::parse("table[align='CENTER'][cellpadding='0']").unwrap();
    let row_selector = Selector::parse("tr").unwrap();
    let td_selector = Selector::parse("td").unwrap();

    let mut title_parts = Vec::new();

    if let Some(table) = document.select(&table_selector).next() {
        for row in table.select(&row_selector) {
            let mut tds = row.select(&td_selector);
            if let (Some(left), Some(right)) = (tds.next(), tds.next()) {
                let left_text = left.text().collect::<String>().trim().to_string();
                let right_text = right.text().collect::<String>().trim().to_string();
                
                if left_text.starts_with("CHAPTER") || left_text.starts_with("SUBCHAPTER") || left_text.starts_with("RULE") {
                    title_parts.push(format!("{}_{}", left_text, right_text));
                }
            }
        }
    }

    let filename = title_parts.join("_").replace(" ", "_");
    let path = format!("data/{}-", &filename);
    let path = Path::new(&path);
    let mut file = File::create(path)?;
    file.write_all(markdown.as_bytes())?;

    Ok((filename, markdown))

}

async fn dedupe_file(file_path: &str) -> Result<String, Box<dyn std::error::Error>>{
    let input_file = File::open(file_path)?;

    let reader = BufReader::new(input_file);

    let mut unique_lines: HashSet<String> = HashSet::new();

    for line in reader.lines() {
        let line = line?;
        unique_lines.insert(line);
    }

    let deduped_file = "deduped_urls.txt".to_string();
    let mut output_file = File::create(&deduped_file)?;

    for line in unique_lines{
        writeln!(output_file, "{}", line)?;
    }

    println!("Unique lines written to {}", &deduped_file);

    Ok(deduped_file)
}

async fn extract_next_url(url: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let resp = reqwest::get(url).await?.text().await?;
    let document = Html::parse_document(&resp);
    let base_url = url.split("readtac").next().unwrap_or("");

    // Selectors for <pre> and <a> tags
    let pre_selector = Selector::parse("body pre").unwrap();
    let a_selector = Selector::parse("a").unwrap();
    let td_selector = Selector::parse("body center table tbody tr td[align='RIGHT']").unwrap();

    // Check for <pre> tags and extract href
    if let Some(pre) = document.select(&pre_selector).next() {
        if let Some(a) = pre
            .select(&a_selector)
            .find(|a| a.text().collect::<String>() == "Next Page")
        {
            if let Some(href) = a.value().attr("href") {
                return Ok(Some(format!("{}{}", base_url, href)));
            }
        }
    }

    // Check for the original td tag and extract href
    for td in document.select(&td_selector) {
        if let Some(a) = td.select(&a_selector).next() {
            if let Some(href) = a.value().attr("href") {
                return Ok(Some(format!("{}{}", base_url, href)));
            }
        }
    }

    Ok(None)
}

async fn add_attached_graphics_urls(
    url: &str,
    all_urls: &mut Vec<String>,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let resp = reqwest::get(url).await?.text().await?;
    let document = Html::parse_document(&resp);
    let base_url = url.split('/').take(3).collect::<Vec<&str>>().join("/");
    let mut graphics_urls = Vec::new();

    // Selector for <a> tags
    let a_selector = Selector::parse("a").unwrap();

    // Iterate through all <a> tags and check their text
    for a in document.select(&a_selector) {
        if a.text().collect::<String>() == "Attached Graphic" {
            if let Some(href) = a.value().attr("href") {
                println!("{}", base_url);
                let attached_graphic_url = format!("{}{}", base_url, href.to_string());
                println!("Attached Graphic! {}", &attached_graphic_url);
                graphics_urls.push(attached_graphic_url.clone());
                all_urls.push(attached_graphic_url.clone());
            }
        }
    }

    Ok(graphics_urls)
}

async fn get_start_url(file_path: &str, default_url: &str) -> String {
    let file = File::open(file_path);

    match file {
        Ok(file) => {
            let reader = BufReader::new(file);

            // Iterate through all lines and capture the last non-empty line
            let mut last_line = String::new();
            for line in reader.lines() {
                if let Ok(content) = line {
                    if !content.trim().is_empty() {
                        last_line = content; // Store the last non-empty line
                    }
                }
            }

            if !last_line.is_empty() {
                last_line
            } else {
                default_url.to_string() // Default if line is empty
            }
        }
        Err(_) => {
            // Return default if the file doesn't exist or any error occurs
            default_url.to_string()
        }
    }
}

// async fn scrape_urls(url: &String) -> Result<Vec<String>, Box<dyn std::error::Error>> {
//
//     let resp = reqwest::get(url).await?.text().await?;
//     let document = Html::parse_document(&resp);
//     let base_url = url.split("readtac").next().unwrap_or("");
//
//     let td_selector = Selector::parse("body center table tbody tr td[align='RIGHT']").unwrap();
//     let a_selector = Selector::parse("a").unwrap();
//
//     let mut urls = Vec::new();
//
//     for td in document.select(&td_selector){
//         if let Some(a) = td.select(&a_selector).next(){
//             if let Some(href) = a.value().attr("href"){
//                 urls.push(format!("{}{}",base_url, href.to_string()));
//             }
//         } else {
//                 break
//             }
//     }
//     println!("{:?}", urls);
//
//     Ok(urls)
// }

// async fn scrape_text_from_urls(
//     urls: Vec<String>,
// ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
//     let client = Client::new();
//     let mut results = Vec::new();
//     for url in urls {
//         let response = client.get(&url).send().await?.text().await?;
//         let document = Html::parse_document(&response);
//
//         let td_selector = Selector::parse("td").unwrap();
//         let a_selector = Selector::parse("a[href^='readtac$ext.ViewTAC']").unwrap();
//
//         let mut text_content = String::new();
//
//         for td in document.select(&td_selector) {
//             if let Some(a) = td.select(&a_selector).next() {
//                 if let Some(href) = a.value().attr("href") {
//                     if href.contains("ch=") || href.contains("pt=") {
//                         text_content.push_str(a.text().collect::<String>().trim());
//                         text_content.push('\n');
//                     }
//                 }
//             } else {
//                 let mut td_text = String::new();
//                 collect_text_excluding_elements(td, &mut td_text, &["script", "form", "input"]);
//                 text_content.push_str(td_text.trim());
//                 text_content.push('\n');
//             }
//         }
//
//         results.push(text_content.trim().to_string());
//     }
//     Ok(results)
// }

// Helper function to collect text excluding certain tags
// fn collect_text_excluding_elements(
//     node: ego_tree::NodeRef,
//     text_accumulator: &mut String,
//     excluded_tags: &[&str],
// ) {
//     for child in node.children() {
//         match child.value() {
//             scraper::node::Node::Text(text) => {
//                 text_accumulator.push_str(text);
//             }
//             scraper::node::Node::Element(element) => {
//                 if !excluded_tags.contains(&element.name()) {
//                     collect_text_excluding_elements(child, text_accumulator, excluded_tags);
//                 }
//             }
//             _ => {}
//         }
//     }
// }

// async fn scrape_text_from_urls(
//     urls: Vec<String>,
// ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
//     let client = Client::new();
//     let mut results = Vec::new();
//     for url in urls {
//         let response = client.get(&url).send().await?.text().await?;
//         let document = Html::parse_document(&response);
//
//         let td_selector = Selector::parse("td").unwrap();
//         let a_selector = Selector::parse("a[href^='readtac$ext.ViewTAC']").unwrap();
//
//         let mut text_content = String::new();
//
//         for td in document.select(&td_selector) {
//             if let Some(a) = td.select(&a_selector).next() {
//                 if let Some(href) = a.value().attr("href") {
//                     if href.contains("ch=") || href.contains("pt=") {
//                         text_content.push_str(a.text().collect::<String>().trim());
//                         text_content.push('\n');
//                     }
//                 }
//             } else {
//                 let mut td_text = String::new();
//                 for child in td.children() {
//                     match child.value() {
//                         scraper::node::Node::Text(text) => {
//                             // If the child is a text node, add its content to td_text
//                             td_text.push_str(text);
//                         }
//                         scraper::node::Node::Element(element) => {
//                             // If the child is an element, check if it's not a <script>, <form>, or <input> tag
//                             if !["script", "form", "input"].contains(&element.name()) {
//                                 // Recursively collect text from the element
//                                 for descendant in child.descendants() {
//                                     if let Some(text) = descendant.value().as_text() {
//                                         td_text.push_str(text);
//                                     }
//                                 }
//                             }
//                         }
//                         _ => {}
//                     }
//                 }
//                 text_content.push_str(td_text.trim());
//                 text_content.push('\n');
//             }
//         }
//
//         results.push(text_content.trim().to_string());
//     }
//     Ok(results)
// }

#[derive(Debug, Deserialize, Serialize)]
struct GeminiResponse {
    candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Candidate {
    content: Content,
}

#[derive(Debug, Deserialize, Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Part {
    text: String,
}

pub async fn call_gemini(
    prompt: &String,
    context: &String,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let api_key = get_env_var_or_fallback("GOOGLE_API_KEY", "API_KEY")?;
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-flash-latest:generateContent?key={}",
        api_key
    );

    let client = Client::new();
    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        {
                            "text": format!("{},\n\n{}\n\n,{}", prompt, context, prompt)
                        }
                    ]
                }
            ]
        }))
        .send()
        .await?;
    let body = response.text().await?;

    let texts = match extract_text_from_response(&body) {
        Ok(texts) => {
            // for text in &texts {
            //     println!("Extracted: \n {} \n\n", text);
            // }
            texts
        }
        Err(e) => {
            println!("Error extractin: {}", e);
            Vec::new()
        }
    };

    Ok(texts)
}

pub fn extract_text_from_response(
    response_body: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let response: GeminiResponse = serde_json::from_str(response_body)?;

    let texts: Vec<String> = response
        .candidates
        .into_iter()
        .flat_map(|candidate| candidate.content.parts)
        .map(|part| part.text)
        .collect();

    Ok(texts)
}

pub fn get_env_var_or_fallback(var1: &str, var2: &str) -> Result<String, std::env::VarError> {
    match std::env::var(var1) {
        Ok(val) => Ok(val),
        Err(_) => match std::env::var(var2) {
            Ok(val) => Ok(val),
            Err(e) => Err(e),
        },
    }
}
