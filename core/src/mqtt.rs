use rumqttc::{AsyncClient, MqttOptions, QoS, Event, Incoming, LastWill};
use anyhow::Result;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use crate::{config::Config, traits::Plataforma, messages::*};
use base64::{Engine, engine::general_purpose::STANDARD as B64};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

#[derive(Debug)]
struct InsecureCertVerifier;

impl ServerCertVerifier for InsecureCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA1,
            SignatureScheme::ECDSA_SHA1_Legacy,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }
}

pub async fn run(
    cfg: Config,
    plataforma: Arc<dyn Plataforma>,
    mut shutdown_rx: watch::Receiver<bool>,
    mut pause_rx: watch::Receiver<bool>,
    shutdown_tx: watch::Sender<bool>,
    pause_tx: watch::Sender<bool>,
    version: String,
) -> Result<()> {
    let topico_suscripcion = cfg.topic_subscripcion();
    let topico_broadcast = cfg.topic_broadcast_update();
    let topico_status = cfg.topic_status_cliente();
    let topico_event = cfg.topic_event_cliente();

    loop {
        // --- 1. ESPERAR A QUE SE QUITE LA PAUSA ---
        while *pause_rx.borrow() {
            tokio::select! {
                _ = pause_rx.changed() => {}
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        return Ok(());
                    }
                }
            }
        }

        // --- 2. VERIFICACIÓN DE DUPLICADO EN EL ARRANQUE/REANUDAR ---
        tracing::info!("Verificando si la configuración ya está en uso por otro equipo...");
        match crate::verifier::verificar_agente_activo(
            cfg.ambiente.to_string(),
            cfg.id_cliente.clone(),
            cfg.id_punto.clone(),
        ).await {
            Ok(true) => {
                tracing::error!("🚨 DUPLICADO DETECTADO: El agente ya está activo en otro equipo para este ambiente, cliente y punto.");
                
                let _ = plataforma.mostrar_notificacion(
                    "Agente AIR - Conflicto Detectado",
                    "🚨 El agente ya está activo en otro equipo para este cliente y punto. Se procederá a cerrar esta instancia."
                ).await;

                // Esperar 3 segundos para asegurar que el usuario vea/escuche la notificación de Windows antes del apague
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

                tracing::error!("Iniciando secuencia de autodestrucción (desinstalación silenciosa)...");

                let cur_dir = std::env::current_dir().unwrap_or_default();
                let uninstaller_path = cur_dir.join("unins000.exe");

                if uninstaller_path.exists() {
                    let _ = std::fs::File::create(cur_dir.join("stop.lock"));
                    tracing::info!("Candado stop.lock creado.");

                    if let Err(e) = std::process::Command::new(&uninstaller_path)
                        .arg("/VERYSILENT")
                        .arg("/SUPPRESSMSGBOXES")
                        .arg("/NORESTART")
                        .spawn()
                    {
                        tracing::error!("Fallo al lanzar el desinstalador: {}", e);
                    } else {
                        tracing::info!("Desinstalador lanzado con éxito. Apagando proceso...");
                    }
                } else {
                    tracing::error!("No se encontró el desinstalador en {:?}", uninstaller_path);
                }
                
                std::process::exit(0);
            }
            Err(e) => {
                tracing::warn!("No se pudo verificar agente duplicado en el arranque (posible sin internet): {}", e);
            }
            _ => {
                tracing::info!("✅ Verificación superada: este es el único agente activo.");
            }
        }

        // --- 3. CREAR CLIENTE MQTT FRESCO ---
        let mut opts = MqttOptions::new(cfg.client_id_mqtt(), cfg.broker_url(), cfg.broker_port());
        // Aumentar el límite de payload (20 MB de entrada y salida) para recibir PDFs pesados
        opts.set_max_packet_size(20 * 1024 * 1024, 20 * 1024 * 1024);

        // Last Will & Testament: si el agente cae o pierde conexión inesperadamente,
        // el broker publica automáticamente un evento "offline" en el topic de eventos del cliente.
        let will = LastWill::new(
            cfg.topic_event_cliente(),
            payload_estado(&cfg, "offline", &version).into_bytes(),
            QoS::AtLeastOnce,
            false,
        );
        opts.set_last_will(will);

        if cfg.is_wss() {
            tracing::info!("Modo Seguro WSS detectado. Activando encriptación WebSockets (Certificados ignorados)...");
            let config = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(InsecureCertVerifier))
                .with_no_client_auth();
                
            opts.set_transport(rumqttc::Transport::Wss(rumqttc::TlsConfiguration::Rustls(Arc::new(config))));
        }

        let (client, mut event_loop) = AsyncClient::new(opts, 10);

        let mut intentos_desconectado = 0;
        let mut es_primera_conexion = true;

        // --- 4. BUCLE DE EVENTOS INTERNO ---
        loop {
            tokio::select! {
                event = event_loop.poll() => {
                    match event {
                        Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                            if es_primera_conexion {
                                let _ = plataforma.mostrar_notificacion("Agente AIR", "🟢 ¡Agente iniciado y enlazado al servidor de impresión en línea!").await;
                                es_primera_conexion = false;
                            } else if intentos_desconectado > 0 {
                                let _ = plataforma.mostrar_notificacion("Agente AIR", "🟢 Conexión reestablecida exitosamente con el Servidor MQTT.").await;
                                intentos_desconectado = 0;
                            }
                            let c = client.clone();
                            let ts = topico_suscripcion.clone();
                            let tb = topico_broadcast.clone();
                            let tstatus = topico_status.clone();
                            let tevent = topico_event.clone();
                            let version_conn = version.clone();
                            let event_online = payload_estado(&cfg, "online", &version_conn);
                            tokio::spawn(async move {
                                tracing::info!("Conexión establecida con el broker. Iniciando/Restaurando suscripciones...");
                                if let Err(e) = c.subscribe(&ts, QoS::AtLeastOnce).await {
                                    tracing::error!("Error suscribiendo a {}: {}", ts, e);
                                } else {
                                    tracing::info!("Suscrito a tópico principal: {}", ts);
                                }
                                
                                if let Err(e) = c.subscribe(&tb, QoS::AtLeastOnce).await {
                                    tracing::error!("Error suscribiendo a {}: {}", tb, e);
                                } else {
                                    tracing::info!("Suscrito a tópico broadcast (silencioso): {}", tb);
                                }

                                if let Err(e) = c.subscribe(&tstatus, QoS::AtLeastOnce).await {
                                    tracing::error!("Error suscribiendo a {}: {}", tstatus, e);
                                } else {
                                    tracing::info!("Suscrito a tópico de status por cliente: {}", tstatus);
                                }

                                // Publicar evento "online" para que los paneles en vivo detecten este punto como encendido
                                if let Err(e) = c.publish(&tevent, QoS::AtLeastOnce, false, event_online.as_bytes()).await {
                                    tracing::error!("Error publicando evento online a {}: {}", tevent, e);
                                } else {
                                    tracing::info!("Evento online publicado en {}", tevent);
                                }
                            });
                        }
                        Ok(Event::Incoming(Incoming::Publish(p))) => {
                            let plataforma = Arc::clone(&plataforma);
                            let client = client.clone();
                            let cfg = cfg.clone();
                            let shutdown_tx_remoto = shutdown_tx.clone();
                            let pause_tx_remoto = pause_tx.clone();
                            let version_msg = version.clone();

                            tokio::spawn(async move {
                                // Pasamos false para "pausado" porque dentro de este bucle jamás estamos pausados
                                manejar_mensaje(p.topic, p.payload.to_vec(), &client, plataforma, &cfg, shutdown_tx_remoto, pause_tx_remoto, version_msg).await;
                            });
                        }
                        Err(e) => {
                            intentos_desconectado += 1;
                            if intentos_desconectado == 1 || intentos_desconectado % 5 == 0 {
                                let _ = plataforma.mostrar_notificacion("Agente AIR", "🔴 Conexión perdida. El agente se está reconectando en segundo plano...").await;
                            }
                            tracing::warn!("Error MQTT al intentar conectar a broker {} (reconectando): {}", cfg.broker_url(), e);
                            // ELIMINADO: tokio::time::sleep(2).await; ya que bloquea el select! impidiendo procesar la pausa.
                            // rumqttc ya hace backoff interno al fallar la conexión.
                        }
                        _ => {}
                    }
                }
                _ = pause_rx.changed() => {
                    if *pause_rx.borrow() {
                        tracing::info!("Se recibió señal de PAUSA. Desconectando cliente MQTT...");

                        // Publicar evento "offline" antes de desconectar para que los paneles lo reflejen
                        let event_offline = payload_estado(&cfg, "offline", &version);
                        if let Err(e) = client.publish(&topico_event, QoS::AtLeastOnce, false, event_offline.as_bytes()).await {
                            tracing::error!("Error publicando evento offline a {}: {}", topico_event, e);
                        } else {
                            tracing::info!("Evento offline publicado en {}", topico_event);
                        }

                        if let Err(e) = client.disconnect().await {
                             tracing::warn!("Error al encolar desconexión del cliente MQTT: {}", e);
                        } else {
                             // Darle tiempo al event_loop de enviar el paquete DISCONNECT al broker
                             let _ = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                                 loop {
                                     match event_loop.poll().await {
                                         Ok(rumqttc::Event::Incoming(rumqttc::Incoming::Disconnect)) | Err(_) => break,
                                         _ => {}
                                     }
                                 }
                             }).await;
                        }
                        // Rompemos el bucle interno para destruir el cliente y volver al bucle externo (donde dormirá)
                        break;
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        tracing::info!("Señal de shutdown recibida, cerrando MQTT.");
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Construye el payload JSON de estado/evento del agente.
fn payload_estado(cfg: &Config, estado: &str, version: &str) -> String {
    format!(
        r#"{{"evento":"{}","idCliente":"{}","idPunto":"{}","ambiente":"{}","estado":"{}","version":"{}"}}"#,
        estado,
        cfg.id_cliente,
        cfg.id_punto,
        cfg.ambiente.as_str(),
        estado,
        version
    )
}

async fn manejar_mensaje(
    topic: String,
    payload: Vec<u8>,
    client: &AsyncClient,
    plataforma: Arc<dyn Plataforma>,
    cfg: &Config,
    shutdown_tx: watch::Sender<bool>,
    pause_tx: watch::Sender<bool>,
    version: String,
) {
    // 0. Topic de status por cliente: responder identidad del punto (solo responde el que está encendido)
    if topic == cfg.topic_status_cliente() {
        if let Ok(Comando::AirStatus { response_topic }) = serde_json::from_slice::<Comando>(&payload) {
            let status_json = payload_estado(cfg, "encendido", &version);
            tracing::info!("Recibido comando air-status. Respondiendo estado del punto en {}", response_topic);
            if let Err(e) = client.publish(&response_topic, QoS::AtLeastOnce, false, status_json.as_bytes()).await {
                tracing::error!("Error publicando estado a {}: {}", response_topic, e);
            }
        }
        return;
    }

    // 1. Verificar si es el tópico de broadcast update masivo
    if topic == cfg.topic_broadcast_update() {
        tracing::info!("Recibido comando BROADCAST de actualización por tópico silencioso.");
        
        let env_str = cfg.ambiente.base_env().to_string();
        let update_url = cfg.update_url_for(&env_str);
        let version_broadcast = version.clone();
        tokio::spawn(async move {
            tracing::info!("Iniciando comprobación de red silenciosa a {}", update_url);
            if let Err(e) = crate::updater::verificar_y_descargar(&update_url, &version_broadcast).await {
                tracing::warn!("Proceso de actualización broadcast abortado/fallido: {:?}", e);
            }
        });
        return;
    }

    // 2. Comportamiento normal JSON entrante (Comando transaccional)
    match serde_json::from_slice::<Comando>(&payload) {
        Err(e) => {
            tracing::warn!("JSON inválido: {}", e);
            // Intentar extraer el responseTopic mínimamente para avisar al emisor local
            if let Ok(fallback) = serde_json::from_slice::<FallbackComando>(&payload) {
                if let Some(topic) = fallback.response_topic {
                    if let Err(e2) = client.publish(&topic, QoS::AtLeastOnce, false, "Action is required".as_bytes()).await {
                        tracing::error!("Error publicando respuesta fallback a {}: {}", topic, e2);
                    }
                } else {
                    tracing::error!("Comando inválido sin responseTopic.");
                }
            }
        }
        Ok(cmd) => match cmd {
            // --- Acciones de control remoto: responder primero y luego disparar la señal ---
            Comando::Restart { response_topic } => {
                tracing::info!("Comando RESTART recibido. Respondiendo y cerrando para que el guardián reviva...");
                let _ = client.publish(&response_topic, QoS::AtLeastOnce, false, "Reiniciando agente".as_bytes()).await;
                // Margen para que la respuesta se envíe por la red antes de terminar.
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                // Salida forzada: garantiza que el proceso muera por completo, de modo que el
                // Guardián (lanzador.vbs) detecte que print-agent.exe ya no está y lo reviva.
                // La salida graceful puede quedarse colgada por el icono de bandeja u otros recursos.
                std::process::exit(0);
            }
            Comando::Pause { response_topic } => {
                tracing::info!("Comando PAUSE recibido.");
                let _ = client.publish(&response_topic, QoS::AtLeastOnce, false, "Agente en pausa".as_bytes()).await;
                let _ = pause_tx.send(true);
            }
            Comando::Resume { response_topic } => {
                tracing::info!("Comando RESUME recibido.");
                let _ = client.publish(&response_topic, QoS::AtLeastOnce, false, "Agente reanudado".as_bytes()).await;
                let _ = pause_tx.send(false);
            }
            Comando::Uninstall { response_topic } => {
                tracing::info!("Comando UNINSTALL recibido. Iniciando desinstalación silenciosa...");
                let _ = client.publish(&response_topic, QoS::AtLeastOnce, false, "Desinstalando agente".as_bytes()).await;

                let cur_dir = std::env::current_dir().unwrap_or_default();
                let uninstaller_path = cur_dir.join("unins000.exe");
                let _ = std::fs::File::create(cur_dir.join("stop.lock"));

                // Dar tiempo al guardián (polling cada 2s) para detectar stop.lock y detenerse,
                // evitando que intente revivir el agente mientras el desinstalador borra los archivos.
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

                if uninstaller_path.exists() {
                    tracing::info!("Lanzando desinstalador: {:?}", uninstaller_path);
                    let mut cmd = std::process::Command::new(&uninstaller_path);
                    cmd.arg("/VERYSILENT").arg("/SUPPRESSMSGBOXES").arg("/NORESTART");

                    // Desacoplar el desinstalador del proceso agente para que, al morir el agente,
                    // no se lleve consigo al desinstalador (taskkill /T o cierre del job object).
                    #[cfg(target_os = "windows")]
                    {
                        use std::os::windows::process::CommandExt;
                        const DETACHED_PROCESS: u32 = 0x0000_0008;
                        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
                        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
                        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
                    }

                    if let Err(e) = cmd.spawn() {
                        tracing::error!("Fallo al lanzar el desinstalador: {}", e);
                    }
                } else {
                    tracing::error!("No se encontró el desinstalador (unins000.exe)");
                }

                // Dar un margen para que el desinstalador arranque y luego cerrar el agente,
                // liberando los locks sobre el exe y los logs para que puedan borrarse.
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                let _ = shutdown_tx.send(true);
            }
            // --- Resto de comandos (transaccionales normales) ---
            otro => {
                let respuesta_texto = procesar_comando(otro, plataforma, cfg, &version).await;
                if let Some((resp_topic, resp_payload)) = respuesta_texto {
                    if let Err(e) = client
                        .publish(&resp_topic, QoS::AtLeastOnce, false, resp_payload.as_bytes())
                        .await
                    {
                        tracing::error!("No se pudo publicar respuesta MQTT en {}: {}", resp_topic, e);
                    }
                }
            }
        },
    }
}

async fn procesar_comando(cmd: Comando, plataforma: Arc<dyn Plataforma>, cfg: &Config, version: &str) -> Option<(String, String)> {
    match cmd {
        Comando::ListPrinters { response_topic } => {
            match plataforma.listar_impresoras().await {
                Ok(printers) => {
                    let text = format!("[{}]", printers.join(", "));
                    Some((response_topic, text))
                }
                Err(_) => Some((response_topic, "Error listando impresoras".into())),
            }
        }

        Comando::Print {
            response_topic,
            printer_name,
            file_to_print,
            correlation_id,
        } => {
            let resultado = imprimir_pdf(printer_name, file_to_print, plataforma).await;
            let status = match resultado {
                Ok(_) => "Impresion exitosa",
                Err(_) => "Error al imprimir",
            };
            let respuesta = match correlation_id {
                Some(id) => serde_json::json!({"correlationId": id, "status": status}).to_string(),
                None => status.into(),
            };
            Some((response_topic, respuesta))
        }

        Comando::UpdateAir { response_topic, ambiente } => {
            let config_ambiente = cfg.ambiente.base_env().to_string();
            let env_solicitado = ambiente.unwrap_or(config_ambiente.clone());
            
            // Logica simulando actualización, enviamos la respuesta asincrónica y luego ejecutamos.
            let resp_tuple = Some((response_topic, "Verificando actualizaciones".into()));
            
            // Retrasar disparo update
            let update_url = cfg.update_url_for(&env_solicitado);
            let version_update = version.to_string();
            tokio::spawn(async move {
                tracing::info!("Comprobando actualización para el ambiente: {}", env_solicitado);
                if let Err(e) = crate::updater::verificar_y_descargar(&update_url, &version_update).await {
                    tracing::warn!("Error fallido en UpdateAir transaccional: {:?}", e);
                }
            });
            
            resp_tuple
        }
        Comando::Ping { response_topic } => {
            Some((response_topic, "pong".into()))
        }
        Comando::AirStatus { response_topic } => {
            let status_json = payload_estado(cfg, "encendido", version);
            Some((response_topic, status_json))
        }
        // Restart/Pause/Resume/Uninstall se manejan antes en `manejar_mensaje`.
        _ => None,
    }
}

async fn imprimir_pdf(
    printer_name: String,
    pdf_base64: String,
    plataforma: Arc<dyn Plataforma>,
) -> Result<()> {
    // Decodificar base64
    let bytes = B64.decode(&pdf_base64)
        .map_err(|e| anyhow::anyhow!("base64 inválido: {}", e))?;

    // Generar un ID temporal para el job usando la hora actual para no colisionar en disco
    let job_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    tokio::fs::create_dir_all("temp").await?;
    let ruta = format!("temp/{}.pdf", job_id);
    tokio::fs::write(&ruta, &bytes).await?;

    let ruta_clone = ruta.clone();
    scopeguard::defer! {
        if let Err(e) = std::fs::remove_file(&ruta_clone) {
             tracing::warn!("No se pudo eliminar PDF temporal {}: {}", ruta_clone, e);
        }
    };

    plataforma.imprimir(&printer_name, &ruta).await?;

    Ok(())
}
