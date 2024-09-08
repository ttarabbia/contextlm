use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Cursor, Write};
use std::path::Path;

use dotenv::dotenv;
use pdf_extract;
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    // let response = call_gemini(&"tell me about Oauth2".to_string(), &"Oauth2".to_string()).await?;
    // for text in &response {
    //     println!("Extracted: \n {} \n", text);
    // }

    scrape_and_save().await?;

    Ok(())
}

async fn scrape_and_save() -> Result<String, Box<dyn std::error::Error>> {
    let folder = "data".to_string();
    let input_file_path = format!("{}/scraped_urls.txt", &folder);
    let first_page = "https://texreg.sos.state.tx.us/public/readtac$ext.TacPage?sl=T&app=9&p_dir=N&p_rloc=199238&p_tloc=&p_ploc=1&pg=2&p_tac=&ti=30&pt=1&ch=1&rl=1";

    let start_url = get_start_url(&format!("{}/scraped_urls.txt", &folder), &first_page).await;
    let url_file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(input_file_path)?;

    //Loop start
    let mut carrier_url: Option<String> = Some(start_url.clone());

    while carrier_url.is_some() {
        let current_url = carrier_url.unwrap();
        //Reqwest(start_url)
        println!("{}", &current_url);
        let resp = reqwest::get(&current_url).await?.text().await?;
        //Clean HTML(response)
        let document = Html::parse_document(&resp);
        let cleaned_document = clean_html(&document).await?;
        //find_title(&html)
        let filename = find_title(&document).await?;
        //handle_attached_graphics
        handle_attached_graphics(
            &cleaned_document,
            &url_file,
            &filename,
            &folder,
            &current_url,
        )
        .await?;

        //html_to_md(&title, &html)
        let markdown = html_to_md(&cleaned_document).await?;
        //Clean MD()

        //save to file
        save_file_to_path(&filename, &folder, &markdown).await?;

        //Look for next URL
        match extract_next_url(&current_url).await? {
            Some(next_url) => {
                writeln!(&url_file, "{}", current_url)?;
                carrier_url = Some(next_url);
            }
            None => carrier_url = None,
        }
    }

    Ok("Yay".to_string())
}
async fn handle_attached_graphics(
    document: &String,
    mut url_file: &File,
    filename: &String,
    folder: &String,
    url: &String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut graphics_urls = Vec::new();
    let document = Html::parse_document(&document);
    let base_url = url.split('/').take(3).collect::<Vec<&str>>().join("/");

    // Selector for <a> tags
    let a_selector = Selector::parse("a").unwrap();

    // Iterate through all <a> tags and check their text
    for a in document.select(&a_selector) {
        if a.text().collect::<String>() == "Attached Graphic" {
            if let Some(href) = a.value().attr("href") {
                let attached_graphic_url = format!("{}{}", base_url, href.to_string());
                println!("Attached Graphic! {}", &attached_graphic_url);
                if href.ends_with(".pdf") {
                    // Download the PDF if it ends with `.pdf`
                    println!("Downloading PDF: {}", &attached_graphic_url);
                    download_pdf(&attached_graphic_url, &href, &filename).await?;
                } else {
                    // If it's not a PDF, call html_to_markdown
                    println!("Processing HTML as Markdown: {}", &attached_graphic_url);
                    let resp = reqwest::get(&attached_graphic_url).await?.text().await?;
                    let document = Html::parse_document(&resp);
                    let cleaned_document = clean_html(&document).await?;
                    let markdown = html_to_md(&cleaned_document).await?;
                    save_file_to_path(&filename, &folder, &markdown).await?;
                }

                graphics_urls.push(attached_graphic_url.clone());
                writeln!(url_file, "{}", attached_graphic_url)?;
            }
        }
    }

    Ok(())
}

async fn clean_html(document: &scraper::html::Html) -> Result<String, Box<dyn std::error::Error>> {
    let remove_selectors = vec![
        Selector::parse("script").unwrap(),
        Selector::parse("input").unwrap(),
        Selector::parse("style").unwrap(),
        Selector::parse("nav").unwrap(),
        Selector::parse("header").unwrap(),
        Selector::parse("footer").unwrap(),
        Selector::parse("center").unwrap(),
        Selector::parse("noscript").unwrap(),
        Selector::parse("center").unwrap(),
        Selector::parse("center").unwrap(),
        Selector::parse("center").unwrap(),
    ];

    let mut cleaned_document = document.root_element().html();

    for selector in remove_selectors {
        document.select(&selector).for_each(|element| {
            let html_to_remove = element.html();
            cleaned_document = cleaned_document.replace(&html_to_remove, "");
        });
    }

    // println!("{}", cleaned_html);
    Ok(cleaned_document)
}

async fn save_file_to_path(
    filename: &String,
    folder: &String,
    markdown: &String,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = format!("{}/{}.md", folder, &filename);
    let path = Path::new(&path);
    let mut file = File::create(path)?;
    file.write_all(markdown.as_bytes())?;
    Ok(())
}

async fn html_to_md(cleaned_document: &String) -> Result<String, Box<dyn std::error::Error>> {
    let cleaned_document = Html::parse_document(&cleaned_document);
    let content_selector = Selector::parse("body").unwrap();
    let main_content = cleaned_document
        .select(&content_selector)
        .next()
        .ok_or("Could not find main content")?;

    let markdown = html2md::parse_html(&main_content.inner_html());

    Ok(markdown)
}

async fn find_title(document: &scraper::html::Html) -> Result<String, Box<dyn std::error::Error>> {
    let table_selector = Selector::parse("table[align='CENTER']").unwrap();
    let row_selector = Selector::parse("tr").unwrap();
    let td_selector = Selector::parse("td").unwrap();

    let mut title_parts = Vec::new();

    if let Some(table) = document.select(&table_selector).next() {
        for row in table.select(&row_selector) {
            let mut tds = row.select(&td_selector);
            if let (Some(left), Some(right)) = (tds.next(), tds.next()) {
                let left_text = left.text().collect::<String>().trim().to_string();
                let right_text = right.text().collect::<String>().trim().to_string();

                // println!("LEFT: {}", &left_text);
                // println!("RIGHT: {}", &right_text);
                if left_text.contains("CHAPTER")
                    || left_text.contains("SUBCHAPTER")
                    || left_text.contains("RULE")
                {
                    let truncated: String = right_text.chars().take(50).collect();

                    title_parts.push(format!("{}_{}", left_text, truncated));
                }
            }
        }
    }

    let filename = title_parts.join("_").replace(" ", "_");

    Ok(filename)
}

async fn dedupe_file(file_path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let input_file = File::open(file_path)?;

    let reader = BufReader::new(input_file);

    let mut unique_lines: HashSet<String> = HashSet::new();

    for line in reader.lines() {
        let line = line?;
        unique_lines.insert(line);
    }

    let deduped_file = "data/deduped_urls.txt".to_string();
    let mut output_file = File::create(&deduped_file)?;

    for line in unique_lines {
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

    // Look for Next Page and go there first, before going to next Rule
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

    //IF no Next Page found - then go to next rule
    for td in document.select(&td_selector) {
        if let Some(a) = td.select(&a_selector).next() {
            if let Some(href) = a.value().attr("href") {
                return Ok(Some(format!("{}{}", base_url, href)));
            }
        }
    }

    Ok(None)
}

async fn download_pdf(
    url: &str,
    href: &str,
    parent_filename: &String,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = reqwest::get(url).await?;
    let pdf_data = response.bytes().await?;

    let filename = href
        .split('/')
        .last()
        .unwrap_or("unknown")
        .replace(" ", "_");

    let folder = "data".to_string();
    let path = format!("{}-{}", parent_filename, filename);
    let full_path = format!("{}/{}", &folder, &path);
    // println!("Saving PDF to: {}", full_path);

    let mut file = File::create(Path::new(&full_path))?;
    file.write_all(&pdf_data)?;

    let pdf_text = fetch_pdf_as_text(&url).await?;
    save_file_to_path(&path, &folder, &pdf_text).await?;

    Ok(())
}

async fn fetch_pdf_as_text(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Download the PDF from the URL
    let response = reqwest::get(url).await?;
    let bytes = response.bytes().await?;

    // Convert the PDF bytes into text
    // let cursor = Cursor::new(bytes);
    // let mut output = Vec::new();
    match pdf_extract::extract_text_from_mem(&bytes) {
        Ok(output) => Ok(output),
        Err(e) => {
            eprintln!("Error extracting {:?}", e);
            Err(Box::new(e))
        }
    }

}

async fn get_start_url(file_path: &String, default_url: &str) -> String {
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
