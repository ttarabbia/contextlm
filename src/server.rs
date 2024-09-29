use actix_files::NamedFile;
use actix_web::{web, Error, HttpResponse, Responder};
use serde::Deserialize;
use std::process::{Command, Stdio};

use crate::{ask_about_code_and_cite, pull_context};

#[derive(Deserialize)]
pub struct FormData {
    content: String,
}

// Handle POST for the first text box
pub async fn post_ripgrep(form: web::Form<FormData>) -> impl Responder {
    let search_term = &form.content;
    let directory = "data/";

    let output = Command::new("rg")
        .arg("--fuzzy")
        .arg(search_term)
        .arg("--type")
        .arg("md")
        .arg(directory)
        .arg("-nH")
        .output()
        .expect("failed to execute search");

    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout).to_string();
        println!("{}", result);
        return HttpResponse::Ok().body(result);
    } else {
        let error = String::from_utf8_lossy(&output.stderr).to_string();
        return HttpResponse::Ok().body(error);
    }

    // let response = format!("Response from Ripgrep: {}", form.content);
}

// Handle POST for the second text box
pub async fn post_llm(form: web::Form<FormData>) -> impl Responder {
    let question = form.content.clone();
    let code = pull_context(".*").unwrap();

    let response = match ask_about_code_and_cite(&code, question).await {
        Ok(responses) if !responses.is_empty() => responses[0].clone(),
        _ => "Failed, try again".to_string(),
    };

    let response = format!("Response from LLM: {}", response);
    HttpResponse::Ok().body(response)
}

// Serve the static HTML page
pub async fn index() -> Result<NamedFile, Error> {
    NamedFile::open("./static/index.html").map_err(Error::from)
}
