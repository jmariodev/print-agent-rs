use anyhow::{Result, bail, Context};
use sha2::{Sha256, Digest};
use reqwest::Client;
use tokio::io::AsyncWriteExt;

pub async fn verificar_y_descargar(update_url: &str, version_actual: &str) -> Result<bool> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let version_url = format!("{}version.txt", update_url);

    let texto = match client.get(&version_url).send().await {
        Err(e) => {
            tracing::warn!("No se pudo verificar actualizaciones (continuando): {}", e);
            return Ok(false);
        }
        Ok(r) => r.text().await?,
    };

    let partes: Vec<&str> = texto.trim().split_whitespace().collect();
    if partes.len() != 2 {
        bail!("version.txt con formato inválido");
    }

    let (version_nueva, hash_esperado) = (partes[0], partes[1]);

    if version_nueva <= version_actual {
        tracing::info!("Agente actualizado (v{}).", version_actual);
        return Ok(false);
    }

    tracing::info!("Nueva versión disponible: {} → {}", version_actual, version_nueva);

    let exe_url = format!("{}print-agent.exe", update_url);
    let mut respuesta = client.get(&exe_url).send().await
        .context("Error descargando nueva versión")?;

    let tmp_path = "print-agent.new.exe";
    let mut archivo = tokio::fs::File::create(tmp_path).await?;
    let mut hasher = Sha256::new();

    while let Some(chunk) = respuesta.chunk().await? {
        hasher.update(&chunk);
        archivo.write_all(&chunk).await?;
    }
    archivo.flush().await?;

    let hash = hasher.finalize();
    let hex = hash.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    if hex != hash_esperado {
        tokio::fs::remove_file(tmp_path).await.ok();
        bail!("Hash SHA256 no coincide — posible ataque MITM. Descarga abortada.");
    }

    tracing::info!("Hash verificado. Lanzando actualización...");
    orquestar_reemplazo().await?;

    Ok(true)
}

async fn orquestar_reemplazo() -> Result<()> {
    let bat = r#"@echo off
timeout /t 3 /nobreak > NUL
move /Y "C:\PrintAgent\print-agent.new.exe" "C:\PrintAgent\print-agent.exe"
sc start PrintAgentRS
del "C:\PrintAgent\update.bat"
"#;

    tokio::fs::write("update.bat", bat).await
        .context("No se pudo escribir update.bat")?;

    std::process::Command::new("cmd")
        .args(["/C", "update.bat"])
        .spawn()
        .context("No se pudo lanzar update.bat")?;

    std::process::exit(0);
}
