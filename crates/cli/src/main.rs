use clap::{Parser, Subcommand};
use intake_core::config::Config;
use intake_core::ops;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "intake", about = "Service de gestion d'intake d'incident (S3)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Crée une affaire : buckets, versioning, site, clé scopée.
    CreateCase { id: String },
    /// Récupère les données d'une affaire.
    PullCase {
        id: String,
        #[arg(long, default_value = "pulled")]
        dest: String,
    },
    /// Détruit une affaire : clé scopée + buckets.
    TeardownCase {
        id: String,
        #[arg(long)]
        yes: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = Config::from_env();

    match cli.cmd {
        Cmd::CreateCase { id } => {
            let r = ops::create_case(&cfg, &id).await?;
            // Sortie : URL du site + les 3 valeurs à remettre à la victime (hors-bande).
            println!("case_id: {}", r.case.id);
            println!("site_url: {}", r.case.site_url);
            println!("data_bucket: {}", r.case.data_bucket);
            println!("--- credentials à remettre à la victime (hors-bande) ---");
            println!("access_key: {}", r.case.scoped_access_key);
            println!("secret_key: {}", r.scoped_secret_key);
            println!("session_token: (aucun sur MinIO)");
        }
        Cmd::PullCase { id, dest } => {
            let files = ops::pull_case(&cfg, &id, &PathBuf::from(&dest)).await?;
            println!("{} objet(s) récupéré(s) dans {dest} :", files.len());
            for (k, _) in files {
                println!("  {k}");
            }
        }
        Cmd::TeardownCase { id, yes } => {
            if !yes {
                anyhow::bail!(
                    "Refus : ajouter --yes pour confirmer la destruction de l'affaire '{id}'."
                );
            }
            ops::teardown_case(&cfg, &id).await?;
            println!("Affaire '{id}' détruite (clé + buckets).");
        }
    }
    Ok(())
}
