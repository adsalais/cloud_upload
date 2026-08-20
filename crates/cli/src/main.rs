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
    /// Récupère les données d'une affaire (option : vérification d'intégrité).
    PullCase {
        id: String,
        #[arg(long, default_value = "pulled")]
        dest: String,
        /// Manifeste SHA-256 (obtenu hors-bande) à vérifier contre les données reçues.
        #[arg(long)]
        manifest: Option<String>,
    },
    /// Détruit une affaire : clé scopée + buckets.
    TeardownCase {
        id: String,
        #[arg(long)]
        yes: bool,
    },
    /// Calcule le manifeste SHA-256 de fichiers/dossiers locaux (data-manifest.json).
    Manifest {
        /// Fichiers ou dossiers à hacher.
        paths: Vec<String>,
        /// Fichier de sortie (défaut : stdout).
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
            // Sortie : URL du site + les 3 valeurs à remettre à la victime (hors-bande).
            println!("case_id: {}", r.case.id);
            println!("site_url: {}", r.case.site_url);
            println!("bucket: {}", r.case.bucket);
            println!("--- credentials à remettre à la victime (hors-bande) ---");
            println!("access_key: {}", r.case.scoped_access_key);
            println!("secret_key: {}", r.scoped_secret_key);
            println!("session_token: (aucun sur MinIO)");
        }
        Cmd::PullCase { id, dest, manifest } => {
            let files = ops::pull_case(&cfg, &id, &PathBuf::from(&dest)).await?;
            println!("{} objet(s) récupéré(s) dans {dest} :", files.len());
            for (k, _) in &files {
                println!("  {k}");
            }
            if let Some(mpath) = manifest {
                use intake_core::integrity;
                let man = integrity::load_manifest(&PathBuf::from(&mpath))?;
                let report = integrity::verify(&files, &man)?;
                println!("--- vérification d'intégrité (SHA-256) ---");
                for (k, label) in &report.matched {
                    println!("  OK         {k}  ↔  {label}");
                }
                for (label, h) in &report.missing {
                    println!("  MANQUANT   {label}  (attendu {}… non reçu)", short(h));
                }
                for (k, h) in &report.unexpected {
                    println!("  INATTENDU  {k}  (sha256 {}… absent du manifeste)", short(h));
                }
                if report.is_ok() {
                    println!("Intégrité : OK ({} objet(s) vérifié(s))", report.matched.len());
                } else {
                    anyhow::bail!(
                        "Intégrité : ÉCHEC — {} manquant(s), {} inattendu(s)",
                        report.missing.len(),
                        report.unexpected.len()
                    );
                }
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
        Cmd::Manifest { paths, out } => {
            if paths.is_empty() {
                anyhow::bail!("Aucun fichier/dossier fourni.");
            }
            let pbs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
            let m = intake_core::integrity::build_manifest(&pbs)?;
            let json = intake_core::integrity::manifest_to_pretty_json(&m)?;
            match out {
                Some(f) => {
                    std::fs::write(&f, json)?;
                    eprintln!("Manifeste écrit : {f} ({} entrée(s))", m.len());
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
