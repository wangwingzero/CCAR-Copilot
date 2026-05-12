use std::path::PathBuf;

use ccar_copilot_lib::regulation::{export_knowledge_from_app_data, KnowledgeExportRequest};

fn value_after(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find_map(|pair| if pair[0] == name { Some(pair[1].clone()) } else { None })
}

fn default_app_data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("com.wangh.ccarcopilot")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let app_data =
        value_after(&args, "--app-data").map(PathBuf::from).unwrap_or_else(default_app_data_dir);
    let output_dir = value_after(&args, "--output-dir");
    let chunk_chars =
        value_after(&args, "--chunk-chars").and_then(|value| value.parse::<usize>().ok());
    let chunk_overlap =
        value_after(&args, "--chunk-overlap").and_then(|value| value.parse::<usize>().ok());

    let request = KnowledgeExportRequest { output_dir, chunk_chars, chunk_overlap };
    match export_knowledge_from_app_data(app_data, request) {
        Ok(response) => {
            println!("{}", serde_json::to_string_pretty(&response).expect("serialize response"));
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
