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

/// Imprime un PDF (vía GDI) y luego envía el corte (vía Spooler RAW).
/// Esto simula el comportamiento exacto de Java:
pub async fn imprimir_win(nombre_impresora: &str, ruta_pdf: &str) -> Result<()> {
    // Bloquear para evitar que si el broker MQTT manda 2 impresiones, se solapen.
    let _guard = PRINT_MUTEX.lock().await;

    // info!("Imprimiendo PDF (Job 1)...");
    imprimir_pdf_gdi(nombre_impresora, ruta_pdf)?;

    // info!("Enviando comando de corte (Job 2)...");
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

        for (i, page) in document.pages().iter().enumerate() {
            if StartPage(hdc) <= 0 {
                let _ = EndDoc(hdc);
                let _ = DeleteDC(hdc);
                bail!("Fallo StartPage en página {}", i);
            }

            let render_config = PdfRenderConfig::new().set_target_width(ancho_px);
            let bitmap = page.render_with_config(&render_config).context("Fallo render")?;

            let bmp_w = bitmap.width() as i32;
            let bmp_h = bitmap.height() as i32;

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: bmp_w,
                    biHeight: -bmp_h, // top-down
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
                Some(bitmap.as_raw_bytes().as_ptr() as *const std::ffi::c_void),
                &bmi, DIB_RGB_COLORS, SRCCOPY,
            );

            if EndPage(hdc) <= 0 {
                let _ = EndDoc(hdc);
                let _ = DeleteDC(hdc);
                bail!("Fallo EndPage");
            }
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
        // Restablecemos este comando original de Java porque el 0x01 corta directamente sobre el texto.
        let cut_cmd: [u8; 4] = [0x1D, 0x56, 0x42, 0x00];
        let mut bytes_escritos = 0;

        let write_ok = WritePrinter(
            h_printer, cut_cmd.as_ptr() as *const std::ffi::c_void,
            cut_cmd.len() as u32, &mut bytes_escritos,
        ).as_bool();

        if !write_ok || bytes_escritos == 0 {
            warn!("WritePrinter falló");
        } else {
            info!("Corte enviado ({} bytes).", bytes_escritos);
        }

        let _ = EndDocPrinter(h_printer);
        let _ = ClosePrinter(h_printer);
    }
    Ok(())
}


