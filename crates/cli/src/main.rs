use clap::{Parser, Subcommand};
use intake_core::config::Config;
use intake_core::ops;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "intake", about = "Discreet incident-data intake (S3)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a case: bucket, versioning, site, scoped key.
    CreateCase { id: String },
    /// Retrieve a case's data (optionally verify integrity).
    PullCase {
        id: String,
        #[arg(long, default_value = "pulled")]
        dest: String,
        /// SHA-256 manifest (obtained out of band) to verify against the received data.
        #[arg(long)]
        manifest: Option<String>,
    },
    /// Destroy a case: scoped key + bucket.
    TeardownCase {
        id: String,
        #[arg(long)]
        yes: bool,
    },
    /// Compute the SHA-256 manifest of local files/directories (data-manifest.json).
    Manifest {
        /// Files or directories to hash.
        paths: Vec<String>,
        /// Output file (default: stdout).
        #[arg(long)]
        out: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = Config::from_env();

    match cli.cmd {
        Cmd::CreateCase { id } => {
            let r = ops::create_case(&cfg, &id).await?;
            // Output: the site URL + the 3 values to hand to the client (out of band).
            println!("case_id: {}", r.case.id);
            println!("site_url: {}", r.case.site_url);
            println!("endpoint: {}", cfg.endpoint);
            println!("bucket: {}", r.case.bucket);
            println!("--- credentials to hand to the client (out of band) ---");
            println!("access_key: {}", r.case.scoped_access_key);
            println!("secret_key: {}", r.scoped_secret_key);
            println!("session_token: (none on MinIO)");
        }
        Cmd::PullCase { id, dest, manifest } => {
            let files = ops::pull_case(&cfg, &id, &PathBuf::from(&dest)).await?;
            println!("{} object(s) retrieved into {dest}:", files.len());
            for (k, _) in &files {
                println!("  {k}");
            }
            if let Some(mpath) = manifest {
                use intake_core::integrity;
                let man = integrity::load_manifest(&PathBuf::from(&mpath))?;
                let report = integrity::verify(&files, &man)?;
                println!("--- integrity check (SHA-256) ---");
                for (k, label) in &report.matched {
                    println!("  OK          {k}  <->  {label}");
                }
                for (label, h) in &report.missing {
                    println!("  MISSING     {label}  (expected {}... not received)", short(h));
                }
                for (k, h) in &report.unexpected {
                    println!("  UNEXPECTED  {k}  (sha256 {}... not in manifest)", short(h));
                }
                if report.is_ok() {
                    println!("Integrity: OK ({} object(s) verified)", report.matched.len());
                } else {
                    anyhow::bail!(
                        "Integrity: FAILED - {} missing, {} unexpected",
                        report.missing.len(),
                        report.unexpected.len()
                    );
                }
            }
        }
        Cmd::TeardownCase { id, yes } => {
            if !yes {
                anyhow::bail!("Refused: pass --yes to confirm destroying case '{id}'.");
            }
            ops::teardown_case(&cfg, &id).await?;
            println!("Case '{id}' destroyed (key + bucket).");
        }
        Cmd::Manifest { paths, out } => {
            if paths.is_empty() {
                anyhow::bail!("No files/directories provided.");
            }
            let pbs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
            let m = intake_core::integrity::build_manifest(&pbs)?;
            let json = intake_core::integrity::manifest_to_pretty_json(&m)?;
            match out {
                Some(f) => {
                    std::fs::write(&f, json)?;
                    eprintln!("Manifest written: {f} ({} entries)", m.len());
                }
                None => println!("{json}"),
            }
        }
    }
    Ok(())
}

fn short(h: &str) -> &str {
    &h[..h.len().min(12)]
}
