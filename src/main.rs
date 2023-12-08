use std::{net::{SocketAddr, Ipv4Addr}, fs::File, io::{BufReader, BufRead}, process::Command};
use axum::{Router, routing::get};
use serde::{Serialize, Deserialize};
use minijinja::render;
use std::io::Write;

/// Template for metric
const HTML_TEMPLATE: &'static str = r#"
{% for metric in metrics %}
{{ metric.body }}
{% endfor %}
"#;

const DEFAULT_CONFIG: &'static str = r#"
{
    "bind": "0.0.0.0",
    "port": 9978,
    "metrics_path": "/metrics",
    "folders": [
        "/tmp"
    ]
}"#;

const LOOP_INTERVAL: u64 = 60; 
const METRICS_FILE: &str = "/tmp/folder-size-exporter.tmp";

/// Metrics structure
#[derive(Debug, Serialize)]
struct Metrics {
    body: String,
}

#[derive(Serialize, Deserialize)]
struct ConfigValues {
    bind: Ipv4Addr,
    port: u16,
    metrics_path: String,
    folders: Vec<String>,
}

fn get_dir_size(path: &String) -> String {
    let cmd = format!("du -s {} | cut -f1", path);
    match Command::new("bash").arg("-c").arg(cmd).output() {
        Ok(o) => {
            return String::from_utf8(o.stdout).unwrap().strip_suffix("\n").unwrap().to_string()
        },
        Err(_) => {
            return "0".to_string()
        },
    };
}

fn get_hostname() -> String {
    let cmd = format!("hostname");
    match Command::new("bash").arg("-c").arg(cmd).output() {
        Ok(o) => {
            return String::from_utf8(o.stdout).unwrap().strip_suffix("\n").unwrap().to_string()
        },
        Err(_) => {
            return "noname".to_string()
        },
    };
}

fn calculate_folders_size_and_create_metrics(folders: Vec<String>, instanse_name: String) {
    loop {
        let mut collected_metrics: Vec<Metrics> = vec![];

        for folder in &folders {
            if std::path::Path::new(folder).exists() {
                let size = get_dir_size(folder);
                let metric = format!("folder_size{{host=\"{}\", folder=\"{}\"}} {}", instanse_name, folder, size);
                collected_metrics.push(Metrics {
                    body: (metric), 
                });
            }
        }

        // Recreate file with collected metrics
        let mut metrics_file = File::create(METRICS_FILE).unwrap();
        for i in &collected_metrics {
            writeln!(metrics_file, "{}", i.body).unwrap();
        }

        // Wait before next try
        std::thread::sleep(tokio::time::Duration::from_secs(LOOP_INTERVAL));
    }
}

/// Read the temporary file with list of all metrics and render the ouput
async fn render_metrics() -> String {
    let mut metrics_vec: Vec<Metrics> = vec![];

    match File::open(METRICS_FILE) {
        Ok(file) => {
            let reader = BufReader::new(file);
            for line in reader.lines() {
                let metric_line = line.unwrap();
                if !metric_line.is_empty() {
                    metrics_vec.push(Metrics {
                        body: (metric_line.to_string()),
                    });
                }
            }
        },
        Err(e) => {
            println!("Error! {}", e);
        },
    }
    
    let contents = render!(HTML_TEMPLATE, metrics => metrics_vec);
    return contents
}

/// Main function.
#[tokio::main]
async fn main() {
    // Load json file
    let json: String = match std::fs::read_to_string("/etc/folder-size-exporter/config.json") {
        Ok(json_result) => {
            json_result
        },
        Err(e) => {
            println!("Error! {:?}", e);
            println!("Loading default configuration...");
            DEFAULT_CONFIG.to_string()
        }
    };

    // Fill struct of values from the json
    let config_values: ConfigValues = serde_json::from_str(&json).unwrap();

    // get hostname and set as instanse label
    let instanse_name = get_hostname();

    // Start main threat to collect metrics
    tokio::task::spawn_blocking(move || calculate_folders_size_and_create_metrics(config_values.folders, instanse_name));

    // Print out how the server is startiing
    println!("Starting server at: http://{}:{}{}", config_values.bind, config_values.port, config_values.metrics_path);

    // Expose metrics
    let addr = SocketAddr::from((config_values.bind, config_values.port));
    let router = Router::new().route(config_values.metrics_path.as_str(), get(render_metrics));
    axum_server::Server::bind(addr)
        .serve(router.into_make_service())
        .await
        .unwrap();
}
