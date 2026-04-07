use anyhow::{Result, Context, bail};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

/// Lista las impresoras instaladas en el sistema usando WMIC.
pub async fn listar_impresoras_win() -> Result<Vec<String>> {
    // Comando para obtener solo el nombre de las impresoras
    let output = Command::new("wmic")
        .args(["printer", "get", "name"])
        .output()
        .await
        .context("Falló al ejecutar wmic para listar impresoras")?;

    if !output.status.success() {
        bail!("wmic terminó con código de error");
    }

    // Convertir salida de bytes (UTF-16 a veces en Windows) a String
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Procesar líneas: omitir el encabezado "Name" y limpiar espacios en blanco
    let impresoras: Vec<String> = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && l.to_lowercase() != "name")
        .collect();

    Ok(impresoras)
}

/// Envía un PDF a imprimir usando SumatraPDF.exe.
pub async fn imprimir_win(nombre_impresora: &str, ruta_pdf: &str) -> Result<()> {
    // SumatraPDF.exe debe estar en el working directory o en el PATH
    let fut = Command::new("SumatraPDF.exe")
        .args([
            "-print-to", nombre_impresora,
            "-silent",
            ruta_pdf,
        ])
        .status();

    let status = timeout(Duration::from_secs(30), fut)
        .await
        .context("Timeout: SumatraPDF no respondió en 30 segundos")?
        .context("Error al ejecutar SumatraPDF.exe")?;

    if !status.success() {
        bail!("SumatraPDF terminó con código de error: {:?}", status.code());
    }

    Ok(())
}
