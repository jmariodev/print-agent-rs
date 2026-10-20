use anyhow::{Result, bail, Context};
use sha2::{Sha256, Digest};
use reqwest::Client;
use tokio::io::AsyncWriteExt;
use std::time::{SystemTime, UNIX_EPOCH};

fn es_version_mayor(nueva: &str, actual: &str) -> bool {
    let parse_nums = |v: &str| -> Vec<u32> {
        v.split('.')
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .collect()
    };
    let v_nueva = parse_nums(nueva);
    let v_actual = parse_nums(actual);

    if v_nueva.is_empty() || v_actual.is_empty() {
        return nueva > actual;
    }

    for (n, a) in v_nueva.iter().zip(v_actual.iter()) {
        if n > a {
            return true;
        } else if n < a {
            return false;
        }
    }
    v_nueva.len() > v_actual.len()
}

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

    if !es_version_mayor(version_nueva, version_actual) {
        tracing::info!("Agente actualizado (v{}).", version_actual);
        return Ok(false);
    }

    tracing::info!("Nueva versión disponible: {} → {}", version_actual, version_nueva);

    let cur_dir = std::env::current_dir()?;

    // Limpiar instaladores temporales de intentos previos para no acumular basura en disco
    if let Ok(mut entries) = tokio::fs::read_dir(&cur_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if (file_name.starts_with("PrintAgentRS_Update_") && file_name.ends_with(".exe"))
                || file_name == "PrintAgentRS_Installer.tmp.exe"
            {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }
    }

    // Descargar el Ejecutable del Instalador (Inno Setup)
    let exe_url = format!("{}PrintAgentRS_Installer.exe", update_url);
    let mut respuesta = client.get(&exe_url).send().await
        .context("Error descargando instalador de actualización")?;

    // Construir la ruta absoluta explícitamente para evitar problemas de PATH en Windows
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

    tracing::info!("Hash verificado. Preparando actualización silenciosa OTA...");
    
    // Renombrar temporal al nombre final con sufijo único (versión + timestamp),
    // evitando colisionar con un instalador de un intento previo que aún esté bloqueado (os error 32).
    let uniq = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let final_exe = cur_dir.join(format!("PrintAgentRS_Update_{}_{}.exe", version_nueva, uniq));
    tokio::fs::rename(&tmp_path, &final_exe).await?;

    tracing::info!("Ruta absoluta resuelta para ejecución: {:?}", final_exe);

    // CRÍTICO: Crear 'stop.lock' antes de lanzar el instalador y salir.
    // Esto previene que el guardián 'lanzador.vbs' detecte el cierre de print-agent.exe
    // y lo reviva antes de que el instalador de Inno Setup pueda sobreescribir el archivo.
    // El nuevo print-agent.exe eliminará 'stop.lock' automáticamente al arrancar.
    let stop_lock_path = cur_dir.join("stop.lock");
    if let Err(e) = tokio::fs::write(&stop_lock_path, "update").await {
        tracing::warn!("No se pudo crear stop.lock antes del update: {}", e);
    } else {
        tracing::info!("Candado stop.lock creado para suspender el guardián durante la actualización.");
    }

    // Ejecutar de manera desatendida el Instalador garantizando que invoque elevación UAC nativa
    std::process::Command::new(&final_exe)
        .arg("/VERYSILENT")
        .arg("/SUPPRESSMSGBOXES")
        .arg("/NORESTART")
        .spawn()
        .context("No se pudo iniciar el instalador silencioso OTA")?;

    // Suicidio del proceso padre para destrabar los archivos antes de que el Setup sobreescriba todo.
    std::process::exit(0);
}
