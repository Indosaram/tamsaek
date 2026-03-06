use std::path::PathBuf;
use clap::{Parser, Subcommand};
use anyhow::Result;
use tracing::error;
use tamsaek_core::{TamsaekIndex, Document};
use walkdir::WalkDir;

/// Tamsaek CLI: A local file search companion
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Provide a custom path for the index (defaults to standard data directory)
    #[arg(short, long, global = true)]
    index_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Perform a full-text search
    Search {
        /// The query to search for
        query: String,
        
        /// Maximum number of results to return
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },
    
    /// Perform a regex search
    SearchRegex {
        /// The regex pattern to search for
        pattern: String,
        
        /// Maximum number of results to return
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },

    /// Index a directory
    Index {
        /// Path to the directory to index
        path: PathBuf,

        /// File extensions to include (e.g., -e rs -e toml or -e rs,toml,md)
        #[arg(short, long, value_delimiter = ',')]
        extensions: Option<Vec<String>>,

        /// Index recursively (defaults to true)
        #[arg(short, long, default_value_t = true)]
        recursive: bool,
    },

    /// Remove a document by its ID
    Remove {
        /// Document ID to remove
        id: String,
    },
    
    /// Filter documents by extension or source
    Filter {
        /// File extension to filter by (e.g. "rs")
        #[arg(short, long)]
        extension: Option<String>,
        
        /// Source to filter by (e.g. "local")
        #[arg(short, long)]
        source: Option<String>,
        
        /// Maximum number of results to return
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },

    /// Retrieve a document by its ID
    GetDocument {
        /// Document ID to retrieve
        id: String,

        /// Show full content (default: truncated to 500 chars)
        #[arg(long)]
        full: bool,
    },

    /// View index statistics
    Stats,

    /// Clear all documents from the index
    Clear {
        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    let index_path = cli.index_path.unwrap_or_else(|| {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tamsaek")
            .join("index")
    });

    if let Some(parent) = index_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let index = TamsaekIndex::open(index_path.clone())?;

    match cli.command {
        Commands::Search { query, limit } => {
            let results = index.search(&query, limit)?;
            println!("Found {} results for '{}':", results.len(), query);
            for (i, r) in results.iter().enumerate() {
                println!("[{}] {} (Score: {:.2})", i + 1, r.path.as_deref().unwrap_or(&r.title), r.score);
                println!("  ID: {}", r.id);
                println!("  Snippet: {}", r.snippet.as_deref().unwrap_or("").replace("\n", " "));
                println!();
            }
        }
        Commands::SearchRegex { pattern, limit } => {
            let results = index.search_regex(&pattern, limit)?;
            println!("Found {} matches for regex '{}':", results.len(), pattern);
            for (i, r) in results.iter().enumerate() {
                println!("[{}] {} (Score: {:.2})", i + 1, r.path.as_deref().unwrap_or(&r.title), r.score);
                println!("  ID: {}", r.id);
                println!("  Snippet: {}", r.snippet.as_deref().unwrap_or("").replace("\n", " "));
                println!();
            }
        }
        Commands::Index { path, extensions, recursive } => {
            if !path.exists() {
                error!("Path does not exist: {}", path.display());
                return Ok(());
            }

            let extensions: Option<Vec<String>> = extensions.map(|exts| {
                exts.into_iter()
                    .map(|e| e.trim_start_matches('.').to_lowercase())
                    .collect()
            });

            let mut indexed = 0;
            let mut failed = 0;
            
            println!("Indexing {}...", path.display());

            let walker = if recursive {
                WalkDir::new(&path)
            } else {
                WalkDir::new(&path).max_depth(1)
            };

            for entry in walker.into_iter().filter_map(|e| e.ok()) {
                if !entry.file_type().is_file() {
                    continue;
                }

                let file_path = entry.path();
                let ext = file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase());

                if let Some(ref allowed_exts) = extensions {
                    if let Some(ref file_ext) = ext {
                        if !allowed_exts.contains(file_ext) {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }

                let content = match std::fs::read_to_string(file_path) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to read {}: {}", file_path.display(), e);
                        failed += 1;
                        continue;
                    }
                };

                let mut doc = Document::new(
                    file_path.display().to_string(),
                    file_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    content,
                )
                .with_path(file_path.display().to_string())
                .with_source("local");

                if let Some(ref e) = ext {
                    doc = doc.with_extension(e);
                }

                if let Err(e) = index.add_document(&doc) {
                    error!("Failed to index {}: {}", file_path.display(), e);
                    failed += 1;
                    continue;
                }
                
                // Commit periodically or after each for CLI. Let's do after each for simplicity.
                if let Err(e) = index.commit() {
                    error!("Failed to commit after indexing {}: {}", file_path.display(), e);
                } else {
                    indexed += 1;
                }
            }

            println!("Indexing complete. {} indexed, {} failed.", indexed, failed);
        }
        Commands::Remove { id } => {
            let exists = match index.get_document(&id) {
                Ok(Some(_)) => true,
                Ok(None) => false,
                Err(e) => {
                    error!("Failed to check document: {}", e);
                    return Ok(());
                }
            };
            
            if !exists {
                println!("Document '{}' not found in index.", id);
                return Ok(());
            }
            
            if let Err(e) = index.delete_document(&id) {
                error!("Failed to remove document: {}", e);
            } else if let Err(e) = index.commit() {
                error!("Failed to commit after removal: {}", e);
            } else {
                println!("Removed document: {}", id);
            }
        }
        Commands::Filter { extension, source, limit } => {
            let results = if let Some(ref ext) = extension {
                index.search_by_extension(ext, limit)?
            } else {
                index.list_all(limit)?
            };
            
            let filtered: Vec<_> = if let Some(ref src) = source {
                results.into_iter().filter(|r| r.source.as_deref() == Some(src.as_str())).collect()
            } else {
                results
            };
            
            println!("Found {} results:", filtered.len());
            for (i, r) in filtered.iter().enumerate() {
                println!("[{}] {} (Score: {:.2})", i + 1, r.path.as_deref().unwrap_or(&r.title), r.score);
                println!("  ID: {}", r.id);
            }
        }
        Commands::GetDocument { id, full } => {
            let doc = match index.get_document(&id) {
                Ok(Some(d)) => d,
                Ok(None) => {
                    println!("Document '{}' not found.", id);
                    return Ok(());
                }
                Err(e) => {
                    error!("Failed to get document: {}", e);
                    return Ok(());
                }
            };

            println!("Document: {}", doc.id);
            println!("  Title:     {}", doc.title);
            if let Some(ref p) = doc.path {
                println!("  Path:      {}", p);
            }
            if let Some(ref ext) = doc.extension {
                println!("  Extension: {}", ext);
            }
            println!("  Source:    {}", doc.source);
            if let Some(ref m) = doc.modified_at {
                println!("  Modified:  {}", m);
            }
            println!("  Size:      {} bytes", doc.content.len());
            println!();

            if full {
                println!("--- Content ---");
                println!("{}", doc.content);
            } else {
                let preview: String = doc.content.chars().take(500).collect();
                let truncated = doc.content.len() > 500;
                println!("--- Content (preview) ---");
                println!("{}", preview);
                if truncated {
                    println!("... (truncated, use --full to see all)");
                }
            }
        }
        Commands::Stats => {
            let doc_count = index.num_docs();
            println!("Index Statistics:");
            println!("  Path: {}", index_path.display());
            println!("  Documents: {}", doc_count);
        }
        Commands::Clear { force } => {
            if !force {
                println!("Are you sure you want to clear the index? This cannot be undone. [y/N]");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Aborted clearing index.");
                    return Ok(());
                }
            }
            let count = index.num_docs();
            if let Err(e) = index.clear() {
                error!("Failed to clear index: {}", e);
            } else {
                println!("Cleared index. Removed {} documents.", count);
            }
        }
    }

    Ok(())
}
