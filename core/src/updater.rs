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

    // Descargar el Ejecutable del Instalador (Inno Setup)
    let exe_url = format!("{}PrintAgentRS_Installer.exe", update_url);
    let mut respuesta = client.get(&exe_url).send().await
        .context("Error descargando instalador de actualización")?;

    // Construir la ruta absoluta explícitamente para evitar problemas de PATH en Windows
    let cur_dir = std::env::current_dir()?;
    let tmp_path = cur_dir.join("PrintAgentRS_Installer.tmp.exe");
    let mut archivo = tokio::fs::File::create(&tmp_path).await?;
    let mut hasher = Sha256::new();

    while let Some(chunk) = respuesta.chunk().await? {
        hasher.update(&chunk);
        archivo.write_all(&chunk).await?;
    }
    archivo.flush().await?;
    
    // IMPORTANTE: Liberar el archivo en el OS. Si el archivo sigue abierto con permisos 
    // de escritura, Windows bloqueará la ejecución (Os Error 32: Sharing Violation).
    drop(archivo);

    let hash = hasher.finalize();
    let hex = hash.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
        
    if hex != hash_esperado {
        tokio::fs::remove_file(&tmp_path).await.ok();
        bail!("Hash SHA256 no coincide — posible ataque MITM o descarga corrupta. Descarga abortada.");
    }

    tracing::info!("Hash verificado. Lanzando actualización silenciosa OTA...");
    
    // Renombrar temporal al final (Ruta absoluta)
    let final_exe = cur_dir.join("PrintAgentRS_Update.exe");
    tokio::fs::rename(&tmp_path, &final_exe).await?;

    tracing::info!("Ruta absoluta resuelta para ejecución: {:?}", final_exe);

    // Ejecutar de manera desatendida el Instalador garantizando que invoque elevación UAC nativa
    std::process::Command::new(&final_exe)
        .arg("/VERYSILENT")
        .arg("/SUPPRESSMSGBOXES")
        .arg("/NORESTART")
        .spawn()
        .context("No se pudo iniciar el instalador silencioso OTA")?;

    // Suicidio del proceso padre para destrabar los archivos antes de que el Setup sobreescriba todo.
    // Inno Setup usará TaskKill de todas formas gracias a nuestro parche previo.
    std::process::exit(0);
}
