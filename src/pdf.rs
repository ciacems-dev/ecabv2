//! HTML → PDF avec un Chromium sans interface (remplace dompdf).
//! Le même HTML que la fiche d'origine est rendu, puis imprimé en A4 portrait.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use rand::RngCore;

pub async fn html_to_pdf(chrome_bin: &str, html: &str) -> Result<Vec<u8>> {
    let mut rnd = [0u8; 8];
    rand::rng().fill_bytes(&mut rnd);
    let dir = std::env::temp_dir().join(format!("ecab-pdf-{}", hex::encode(rnd)));
    tokio::fs::create_dir_all(&dir).await?;
    let input = dir.join("fiche.html");
    let output = dir.join("fiche.pdf");
    tokio::fs::write(&input, html).await?;

    let run = tokio::process::Command::new(chrome_bin)
        .arg("--headless=new")
        .arg("--disable-gpu")
        .arg("--no-sandbox") // conteneur sans espace de noms utilisateur ; le HTML est généré par l'application
        .arg("--disable-dev-shm-usage")
        .arg("--no-pdf-header-footer")
        .arg(format!("--user-data-dir={}", dir.join("profile").display()))
        .arg(format!("--print-to-pdf={}", output.display()))
        .arg(format!("file://{}", input.display()))
        .kill_on_drop(true)
        .output();
    let result = tokio::time::timeout(Duration::from_secs(30), run).await;

    let pdf = async {
        let out = result.context("génération du PDF trop longue")?.context("lancement de Chromium")?;
        if !out.status.success() {
            bail!("Chromium a échoué : {}", String::from_utf8_lossy(&out.stderr));
        }
        tokio::fs::read(&output).await.context("lecture du PDF généré")
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&dir).await;
    pdf
}
