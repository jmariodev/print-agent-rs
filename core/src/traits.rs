use async_trait::async_trait;
use anyhow::Result;

#[async_trait]
pub trait Plataforma: Send + Sync {
    async fn listar_impresoras(&self) -> Result<Vec<String>>;
    async fn imprimir(&self, nombre: &str, ruta_pdf: &str) -> Result<()>;
    async fn impresora_predeterminada(&self) -> Result<Option<String>>;
}
