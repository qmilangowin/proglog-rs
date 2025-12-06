use crate::{
    errors::{LogError, NetworkError},
    storage::log::Log,
};
use proto::{ConsumeRequest, ConsumeResponse, ProduceRequest, ProduceResponse};
use std::sync::{Arc, Mutex};
use tonic::{Request, Response, Status};

pub mod proto {
    tonic::include_proto!("log.v1");
}

trait IntoStatus {
    fn into_status(self) -> Status;
}

impl IntoStatus for LogError {
    fn into_status(self) -> Status {
        match &self {
            LogError::OffsetNotFound { offset, .. } => {
                Status::not_found(format!("Offset {offset} not found"))
            }
            LogError::Segment(e) => Status::internal(format!("Segment error: {e}")),
            _ => Status::internal("Log error: {self}"),
        }
    }
}

impl IntoStatus for NetworkError {
    fn into_status(self) -> Status {
        match &self {
            NetworkError::LockPoisoned => Status::internal("Lock poisoned"),
            NetworkError::TaskFailed(msg) => Status::internal(format!("Task failed: {msg}")),
            _ => Status::internal(format!("Network error: {self}")),
        }
    }
}
pub struct LogService {
    log: Arc<Mutex<Log>>,
}

impl LogService {
    pub fn new(log: Log) -> Self {
        Self {
            log: Arc::new(Mutex::new(log)),
        }
    }
}

#[tonic::async_trait]
impl proto::log_server::Log for LogService {
    async fn produce(
        &self,
        request: Request<ProduceRequest>,
    ) -> Result<Response<ProduceResponse>, Status> {
        let record = request.into_inner().record;
        let log = Arc::clone(&self.log);

        // Run blocking op on thread-pool
        let offset = tokio::task::spawn_blocking(move || {
            let mut log = log
                .lock()
                .map_err(|_| NetworkError::LockPoisoned.into_status())?;

            log.append(&record).map_err(|e| e.into_status())
        })
        .await
        .map_err(|e| NetworkError::TaskFailed(e.to_string()).into_status())??;

        Ok(Response::new(ProduceResponse { offset }))
    }

    async fn consume(
        &self,
        request: Request<ConsumeRequest>,
    ) -> Result<Response<ConsumeResponse>, Status> {
        let offset = request.into_inner().offset;
        let log = Arc::clone(&self.log);

        let record = tokio::task::spawn_blocking(move || {
            let log = log
                .lock()
                .map_err(|_| NetworkError::LockPoisoned.into_status())?;

            log.read(offset).map_err(|e| e.into_status())
        })
        .await
        .map_err(|e| NetworkError::TaskFailed(e.to_string()).into_status())??;

        Ok(Response::new(ConsumeResponse { record, offset }))
    }
}
