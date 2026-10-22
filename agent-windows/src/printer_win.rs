use anyhow::{Result, Context, bail};
use tracing::{info, warn};

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::Xps::*;
use windows::Win32::Graphics::Printing::*;
use pdfium_render::prelude::*;

// Mutex global para garantizar que las impresiones no se solapen si llegan muy rápido por MQTT.
static PRINT_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Lista las impresoras instaladas en el sistema usando la API nativa de Windows (EnumPrintersW).
/// Esto evita depender de PowerShell o procesos externos (WMIC), mejorando el rendimiento a milisegundos.
pub async fn listar_impresoras_win() -> Result<Vec<String>> {
    unsafe {
        let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
        let mut cb_needed = 0;
        let mut c_returned = 0;

        // Primera llamada para obtener tamaño necesario en bytes
        let _ = EnumPrintersW(flags, PCWSTR::null(), 4, None, &mut cb_needed, &mut c_returned);

        if cb_needed == 0 {
            return Ok(Vec::new());
        }

        let mut buf: Vec<u8> = vec![0; cb_needed as usize];

        if EnumPrintersW(
            flags,
            PCWSTR::null(),
            4,
            Some(buf.as_mut_slice()),
            &mut cb_needed,
            &mut c_returned,
        ).is_err() {
            bail!("Error nativo al listar impresoras con EnumPrintersW");
        }

        let printers = std::slice::from_raw_parts(
            buf.as_ptr() as *const PRINTER_INFO_4W,
            c_returned as usize,
        );

        let mut nombres = Vec::new();
        for printer in printers {
            if !printer.pPrinterName.is_null() {
                if let Ok(nombre) = printer.pPrinterName.to_string() {
                    if !nombre.is_empty() {
                        nombres.push(nombre);
                    }
                }
            }
        }

        Ok(nombres)
    }
}

/// Verifica el estado físico y lógico de la impresora antes de tocar la cola del Spooler.
/// Retorna un error descriptivo si la impresora está apagada, sin papel, atascada o en error.
pub fn verificar_estado_impresora(nombre_impresora: &str) -> Result<()> {
    unsafe {
        let printer_name_w: Vec<u16> = format!("{}\0", nombre_impresora).encode_utf16().collect();
        let mut h_printer = HANDLE::default();
        let mut defaults = PRINTER_DEFAULTSW {
            pDatatype: PWSTR::null(),
            pDevMode: std::ptr::null_mut(),
            DesiredAccess: PRINTER_ACCESS_USE,
        };

        if OpenPrinterW(PCWSTR(printer_name_w.as_ptr()), &mut h_printer, Some(&mut defaults)).is_err() {
            bail!("Error: Impresora '{}' no encontrada en el equipo", nombre_impresora);
        }

        let mut cb_needed = 0;
        let _ = GetPrinterW(h_printer, 2, None, &mut cb_needed);
        if cb_needed == 0 {
            let _ = ClosePrinter(h_printer);
            return Ok(());
        }

        let mut buf = vec![0u8; cb_needed as usize];
        if GetPrinterW(h_printer, 2, Some(buf.as_mut_slice()), &mut cb_needed).is_err() {
            let _ = ClosePrinter(h_printer);
            return Ok(());
        }

        let info = &*(buf.as_ptr() as *const PRINTER_INFO_2W);
        let status = info.Status;
        let attributes = info.Attributes;
        let _ = ClosePrinter(h_printer);

        // Banderas bloqueantes específicas de hardware y sistema
        if status & PRINTER_STATUS_PAPER_OUT != 0 {
            bail!("Error: Impresora sin papel");
        }
        if status & PRINTER_STATUS_OFFLINE != 0 || attributes & PRINTER_ATTRIBUTE_WORK_OFFLINE != 0 {
            bail!("Error: Impresora fuera de línea o desconectada");
        }
        if status & PRINTER_STATUS_PAPER_JAM != 0 {
            bail!("Error: Papel atascado en la impresora");
        }
        if status & PRINTER_STATUS_PAPER_PROBLEM != 0 {
            bail!("Error: Problema con el papel en la impresora");
        }
        if status & PRINTER_STATUS_DOOR_OPEN != 0 {
            bail!("Error: Tapa de la impresora abierta");
        }
        if status & PRINTER_STATUS_PAUSED != 0 {
            bail!("Error: Impresora en pausa");
        }
        if status & PRINTER_STATUS_USER_INTERVENTION != 0 {
            bail!("Error: La impresora requiere intervención del usuario (atención física)");
        }
        if status & PRINTER_STATUS_OUTPUT_BIN_FULL != 0 {
            bail!("Error: Bandeja de salida llena o papel acumulado en la salida");
        }
        if status & PRINTER_STATUS_OUT_OF_MEMORY != 0 {
            bail!("Error: Memoria de la impresora agotada");
        }
        if status & PRINTER_STATUS_ERROR != 0 {
            bail!("Error: La impresora reporta falla de hardware");
        }
        if status & PRINTER_STATUS_NOT_AVAILABLE != 0 {
            bail!("Error: Impresora no disponible");
        }
        if status & PRINTER_STATUS_NO_TONER != 0 {
            bail!("Error: Impresora sin consumible o tóner");
        }
        if status & PRINTER_STATUS_SERVER_OFFLINE != 0 {
            bail!("Error: Servidor de impresión de red fuera de línea");
        }
        if status & PRINTER_STATUS_SERVER_UNKNOWN != 0 {
            bail!("Error: Servidor de impresión de red no disponible");
        }
        if status & PRINTER_STATUS_PENDING_DELETION != 0 {
            bail!("Error: La impresora está en proceso de eliminación");
        }
        if status & PRINTER_STATUS_PAGE_PUNT != 0 {
            bail!("Error: La impresora no puede procesar los datos de la página");
        }

        // Máscara de estados normales de actividad operativa que NO deben bloquear la comanda
        let normal_mask = PRINTER_STATUS_PRINTING
            | PRINTER_STATUS_BUSY
            | PRINTER_STATUS_PROCESSING
            | PRINTER_STATUS_WAITING
            | PRINTER_STATUS_WARMING_UP
            | PRINTER_STATUS_INITIALIZING
            | PRINTER_STATUS_POWER_SAVE
            | PRINTER_STATUS_TONER_LOW;

        // Comodín para cualquier código de error propietario o no estándar devuelto por el driver
        let abnormal_bits = status & !normal_mask;
        if abnormal_bits != 0 {
            bail!("Error: Estado no operativo en impresora (código: 0x{:08X})", abnormal_bits);
        }

        Ok(())
    }
}

/// Imprime un PDF (vía GDI) y luego envía el corte (vía Spooler RAW).
/// Esto simula el comportamiento exacto de Java:
pub async fn imprimir_win(nombre_impresora: &str, ruta_pdf: &str) -> Result<()> {
    // Bloquear para evitar que si el broker MQTT manda 2 impresiones, se solapen.
    let _guard = PRINT_MUTEX.lock().await;

    // 1. Pre-flight check: verificar estado de la impresora antes de enviar nada al Spooler
    verificar_estado_impresora(nombre_impresora)?;

    // 2. Renderizado GDI y envío al Spooler
    imprimir_pdf_gdi(nombre_impresora, ruta_pdf)?;

    // 3. Envío de corte RAW
    if let Err(e) = enviar_corte_raw(nombre_impresora) {
        warn!("Fallo al enviar corte RAW: {}", e);
    }

    Ok(())
}

fn imprimir_pdf_gdi(nombre_impresora: &str, ruta_pdf: &str) -> Result<()> {
    let pdfium = Pdfium::new(
        Pdfium::bind_to_library(
            Pdfium::pdfium_platform_library_name_at_path("./")
        ).or_else(|_| Pdfium::bind_to_system_library())
        .context("No se encontró pdfium.dll")?
    );

    let document = pdfium.load_pdf_from_file(ruta_pdf, None)
        .context("Fallo al cargar el PDF")?;

    unsafe {
        let printer_name = HSTRING::from(nombre_impresora);
        let hdc = CreateDCW(None, &printer_name, None, None);

        if hdc.is_invalid() {
            bail!("Fallo al crear DC para la impresora");
        }

        let ancho_px = GetDeviceCaps(hdc, HORZRES);
        let doc_name: Vec<u16> = "Ticket\0".encode_utf16().collect();
        let doc_info = DOCINFOW {
            cbSize: std::mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: PCWSTR(doc_name.as_ptr()),
            lpszOutput: PCWSTR::null(),
            lpszDatatype: PCWSTR::null(),
            fwType: 0,
        };

        if StartDocW(hdc, &doc_info) <= 0 {
            let _ = DeleteDC(hdc);
            bail!("Fallo StartDocW");
        }

        let mut error_ocurrido: Option<anyhow::Error> = None;

        for (i, page) in document.pages().iter().enumerate() {
            if StartPage(hdc) <= 0 {
                error_ocurrido = Some(anyhow::anyhow!("Fallo StartPage en página {}", i));
                break;
            }

            let render_config = PdfRenderConfig::new().set_target_width(ancho_px);
            let bitmap = match page.render_with_config(&render_config) {
                Ok(b) => b,
                Err(e) => {
                    error_ocurrido = Some(anyhow::anyhow!("Fallo render en página {}: {}", i, e));
                    break;
                }
            };

            let bgra_bytes = bitmap.as_raw_bytes();
            let bmp_w = bitmap.width() as i32;
            let bmp_h = bitmap.height() as i32;

            // Obtener el umbral de negro desde la configuración global (por defecto 200, Gamma 2.0)
            let umbral = crate::CONFIG_GLOBAL.get()
                .and_then(|c| c.umbral_negro)
                .unwrap_or(200);

            // Invertimos las filas manualmente (Bottom-Up) para que sea compatible con el 100%
            // de los drivers de Windows, incluyendo impresoras antiguas o genéricas (como SAT o Xprinter).
            let mut flipped_bytes = Vec::with_capacity(bgra_bytes.len());
            let row_size = (bmp_w * 4) as usize;
            
            if umbral > 0 && umbral != 100 {
                // Mapeamos el umbral a un valor gamma (ej. umbral de 200 -> gamma de 2.0)
                // Esto aplica una corrección gamma suave en vez de una binarización dura,
                // manteniendo los degradados del logo y la claridad del QR intactos.
                let gamma = umbral as f32 / 100.0;
                let mut lut = [0u8; 256];
                for (i, val) in lut.iter_mut().enumerate() {
                    let normalized = i as f32 / 255.0;
                    let darkened = normalized.powf(gamma);
                    *val = (darkened * 255.0).round().clamp(0.0, 255.0) as u8;
                }

                for row in (0..bmp_h).rev() {
                    let start = (row as usize) * row_size;
                    let end = start + row_size;
                    let row_bytes = &bgra_bytes[start..end];
                    
                    for chunk in row_bytes.chunks_exact(4) {
                        let b = chunk[0];
                        let g = chunk[1];
                        let r = chunk[2];
                        let a = chunk[3];
                        
                        if a < 50 {
                            flipped_bytes.push(255); // B
                            flipped_bytes.push(255); // G
                            flipped_bytes.push(255); // R
                            flipped_bytes.push(255); // A
                        } else {
                            flipped_bytes.push(lut[b as usize]); // B
                            flipped_bytes.push(lut[g as usize]); // G
                            flipped_bytes.push(lut[r as usize]); // R
                            flipped_bytes.push(255);             // A
                        }
                    }
                }
            } else {
                for row in (0..bmp_h).rev() {
                    let start = (row as usize) * row_size;
                    let end = start + row_size;
                    flipped_bytes.extend_from_slice(&bgra_bytes[start..end]);
                }
            }

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: bmp_w,
                    biHeight: bmp_h, // POSITIVO = Bottom-Up (Máxima compatibilidad)
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [RGBQUAD::default(); 1],
            };

            StretchDIBits(
                hdc, 0, 0, bmp_w, bmp_h, 0, 0, bmp_w, bmp_h,
                Some(flipped_bytes.as_ptr() as *const std::ffi::c_void),
                &bmi, DIB_RGB_COLORS, SRCCOPY,
            );

            if EndPage(hdc) <= 0 {
                error_ocurrido = Some(anyhow::anyhow!("Fallo EndPage en página {}", i));
                break;
            }
        }

        // Si ocurrió algún fallo, abortar el trabajo para que Windows lo borre de la cola inmediatamente
        if let Some(err) = error_ocurrido {
            warn!("Fallo en impresión GDI. Abortando trabajo de cola con AbortDoc: {}", err);
            let _ = AbortDoc(hdc);
            let _ = DeleteDC(hdc);
            return Err(err);
        }

        let _ = EndDoc(hdc);
        let _ = DeleteDC(hdc);
    }
    Ok(())
}

fn enviar_corte_raw(nombre_impresora: &str) -> Result<()> {
    unsafe {
        let mut h_printer: HANDLE = HANDLE::default();
        let printer_name_w: Vec<u16> = format!("{}\0", nombre_impresora).encode_utf16().collect();
        
        let mut defaults = PRINTER_DEFAULTSW {
            pDatatype: PWSTR::null(),
            pDevMode: std::ptr::null_mut(),
            DesiredAccess: PRINTER_ACCESS_USE,
        };

        if OpenPrinterW(PCWSTR(printer_name_w.as_ptr()), &mut h_printer, Some(&mut defaults)).is_err() {
            bail!("Fallo OpenPrinterW para el corte");
        }

        let mut doc_name: Vec<u16> = "Corte\0".encode_utf16().collect();
        let mut datatype: Vec<u16> = "RAW\0".encode_utf16().collect();
        
        let doc_info = DOC_INFO_1W {
            pDocName: PWSTR(doc_name.as_mut_ptr()),
            pOutputFile: PWSTR::null(),
            pDatatype: PWSTR(datatype.as_mut_ptr()),
        };

        if StartDocPrinterW(h_printer, 1, &doc_info as *const DOC_INFO_1W) == 0 {
            let _ = ClosePrinter(h_printer);
            bail!("Fallo StartDocPrinterW");
        }

        // Comando ESC/POS de corte (0x42 = Avanzar papel hasta el cuchillo y hacer Partial Cut).
        let cut_cmd: [u8; 4] = [0x1D, 0x56, 0x42, 0x00];
        let mut bytes_escritos = 0;

        let write_ok = WritePrinter(
            h_printer, cut_cmd.as_ptr() as *const std::ffi::c_void,
            cut_cmd.len() as u32, &mut bytes_escritos,
        ).as_bool();

        if !write_ok || bytes_escritos == 0 {
            warn!("WritePrinter falló al enviar corte RAW, abortando trabajo");
            let _ = AbortPrinter(h_printer);
        } else {
            info!("Corte enviado ({} bytes).", bytes_escritos);
            let _ = EndDocPrinter(h_printer);
        }

        let _ = ClosePrinter(h_printer);
    }
    Ok(())
}


