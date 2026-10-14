use crate::{
    config::AgentConfig,
    ip_whitelist::is_client_ip_allowed,
    logs::TaskLogEntry,
    printing::PrintError,
    protocol::{
        is_allowed_origin, validate_html_file_url, ClientMessage, ErrorCode, JobStatus,
        JobValidationError, PrintJobInput, PrintQueueJobInfo, PrinterDetails, ServerMessage,
        SupportedFormat,
    },
    queue::QueueError,
    state::AgentState,
};
use axum::{
    extract::{
        connect_info::ConnectInfo,
        ws::{Message, WebSocket, WebSocketUpgrade},
        Request, State,
    },
    http::{header::ORIGIN, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::HashSet,
    net::{AddrParseError, IpAddr, SocketAddr},
    str::FromStr,
};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
const BIND_HOST: &str = "0.0.0.0";

/// Errors that can occur when binding or starting the local service.
#[derive(Debug, Error)]
pub enum ServerError {
    #[error("server bind failed: {0}")]
    Bind(#[from] std::io::Error),
    #[error("invalid server address: {0}")]
    InvalidAddress(#[from] AddrParseError),
}

/// Construct network router exposing only the PrintBridge WebSocket protocol.
pub fn router(state: AgentState) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            ip_whitelist_middleware,
        ))
        .with_state(state)
}

/// Parse service bind address. Agent binds to all network interfaces for LAN client access.
pub fn configured_addr(config: &AgentConfig) -> Result<SocketAddr, AddrParseError> {
    SocketAddr::from_str(&format!("{}:{}", BIND_HOST, config.service.port))
}

/// Run local service within current task.
pub async fn bind_listener(config: &AgentConfig) -> Result<(SocketAddr, TcpListener), ServerError> {
    let addr = configured_addr(config)?;
    let listener = TcpListener::bind(addr).await?;
    let addr = listener.local_addr()?;
    Ok((addr, listener))
}

/// Run local service using pre-bound listener.
pub async fn serve_listener(state: AgentState, listener: TcpListener) -> Result<(), ServerError> {
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// Run service using pre-bound listener, stopping gracefully upon cancellation signal.
pub async fn serve_listener_until(
    state: AgentState,
    listener: TcpListener,
    shutdown: CancellationToken,
) -> Result<(), ServerError> {
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown.cancelled_owned())
    .await?;
    Ok(())
}

/// Run local service within current task.
pub async fn run_server(state: AgentState) -> Result<(), ServerError> {
    let config = state.config.read().await.clone();
    let (_, listener) = bind_listener(&config).await?;
    serve_listener(state, listener).await
}

/// Upgrade allowed browser connections to PrintBridge WebSocket protocol.
async fn ws_handler(
    State(state): State<AgentState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let origin = headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if !is_ws_origin_allowed(&state, origin.as_deref()).await {
        return StatusCode::FORBIDDEN.into_response();
    }

    ws.on_upgrade(move |socket| handle_socket(state, socket, origin))
}

/// Check whether WebSocket request Origin is allowed by current configuration.
pub async fn is_ws_origin_allowed(state: &AgentState, origin: Option<&str>) -> bool {
    let config = state.config.read().await;
    is_allowed_origin(origin, &config.security.allowed_origins)
}

/// Check whether client IP is allowed by current configuration.
pub async fn is_client_ip_allowed_for_state(state: &AgentState, client_ip: IpAddr) -> bool {
    let config = state.config.read().await;
    is_client_ip_allowed(client_ip, &config.security.allowed_ips)
}

/// Intercept clients not in IP allowlist before all HTTP/WebSocket routes.
async fn ip_whitelist_middleware(
    State(state): State<AgentState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(ConnectInfo(addr)) = request.extensions().get::<ConnectInfo<SocketAddr>>() else {
        return next.run(request).await;
    };

    if is_client_ip_allowed_for_state(&state, addr.ip()).await {
        next.run(request).await
    } else {
        client_ip_error_response()
    }
}

/// Construct HTTP error response when client IP is not in allowlist.
fn client_ip_error_response() -> Response {
    StatusCode::FORBIDDEN.into_response()
}

/// Handle individual WebSocket connection, forwarding only tasks accepted by that connection.
async fn handle_socket(state: AgentState, socket: WebSocket, origin: Option<String>) {
    let (mut sender, mut receiver) = socket.split();
    let mut status_events = state.subscribe_status_events();
    // Each browser connection only receives status events for tasks it submitted.
    let mut accepted_job_ids = HashSet::new();

    loop {
        tokio::select! {
            result = receiver.next() => {
                let Some(result) = result else {
                    break;
                };
                let message = match result {
                    Ok(message) => message,
                    Err(error) => {
                        log::debug!("websocket receive failed: {error}");
                        break;
                    }
                };

                let outcome = match message {
                    Message::Text(text) => handle_client_text(&state, &text, origin.as_deref()).await,
                    Message::Ping(payload) => {
                        if sender.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                        continue;
                    }
                    Message::Close(_) => break,
                    _ => continue,
                };

                accepted_job_ids.extend(outcome.accepted_job_ids);
                match serde_json::to_string(&outcome.response) {
                    Ok(json) => {
                        if sender.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        log::error!(
                            "websocket response serialization failed: {error}"
                        );
                        break;
                    }
                }
            }
            event = status_events.recv() => {
                match event {
                    Ok(entry) => {
                        let Some(response) = status_message_for_connection(&entry, &accepted_job_ids) else {
                            continue;
                        };
                        match serde_json::to_string(&response) {
                            Ok(json) => {
                                if sender.send(Message::Text(json.into())).await.is_err() {
                                    break;
                                }
                            }
                            Err(error) => {
                                log::error!(
                                    "websocket status serialization failed: {error}"
                                );
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        log::debug!(
                            "websocket status receiver skipped {skipped} events"
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

/// Processing result of single client message, including newly bound tasks on this socket.
struct ClientTextOutcome {
    response: ServerMessage,
    accepted_job_ids: Vec<String>,
}

impl ClientTextOutcome {
    /// Construct response result when no new tasks are accepted.
    fn response(response: ServerMessage) -> Self {
        Self {
            response,
            accepted_job_ids: Vec::new(),
        }
    }
}

/// Parse single client text frame and return protocol response.
async fn handle_client_text(
    state: &AgentState,
    text: &str,
    origin: Option<&str>,
) -> ClientTextOutcome {
    let message = match serde_json::from_str::<ClientMessage>(text) {
        Ok(message) => message,
        Err(error) => {
            return ClientTextOutcome::response(ServerMessage::Error {
                request_id: None,
                error_code: ErrorCode::InvalidMessage,
                message: error.to_string(),
            });
        }
    };

    match message {
        ClientMessage::Ping { time } => ClientTextOutcome::response(ServerMessage::Pong {
            time,
            agent_status: "ready".to_string(),
        }),
        ClientMessage::GetPrintersList { request_id } => match state.printing.list_printers() {
            Ok(mut printers) => {
                if let Some(default_printer) =
                    state.config.read().await.printing.default_printer.clone()
                {
                    for printer in &mut printers {
                        printer.is_default = printer.name == default_printer;
                    }
                }

                ClientTextOutcome::response(ServerMessage::PrintersList {
                    request_id,
                    printers,
                })
            }
            Err(error) => {
                ClientTextOutcome::response(print_error_response(Some(request_id), error))
            }
        },
        ClientMessage::GetPrinterInfo {
            request_id,
            printer_name,
        } => {
            let printer = match state.printing.list_printers() {
                Ok(printers) => printers
                    .into_iter()
                    .find(|printer| printer.name == printer_name),
                Err(error) => {
                    return ClientTextOutcome::response(print_error_response(
                        Some(request_id),
                        error,
                    ));
                }
            };

            let Some(printer) = printer else {
                return ClientTextOutcome::response(print_error_response(
                    Some(request_id),
                    PrintError::PrinterNotFound(printer_name),
                ));
            };

            let papers = match state.printing.list_papers(&printer.name) {
                Ok(papers) => papers,
                Err(error) => {
                    return ClientTextOutcome::response(print_error_response(
                        Some(request_id),
                        error,
                    ));
                }
            };
            let trays = match state.printing.list_trays(&printer.name) {
                Ok(trays) => trays,
                Err(error) => {
                    return ClientTextOutcome::response(print_error_response(
                        Some(request_id),
                        error,
                    ));
                }
            };
            let media_types = match state.printing.list_media_types(&printer.name) {
                Ok(media_types) => media_types,
                Err(error) => {
                    return ClientTextOutcome::response(print_error_response(
                        Some(request_id),
                        error,
                    ));
                }
            };

            ClientTextOutcome::response(ServerMessage::PrinterInfo {
                request_id,
                printer: PrinterDetails {
                    name: printer.name,
                    is_default: printer.is_default,
                    dpi: printer.dpi,
                    port: printer.port,
                    is_local: printer.is_local,
                    is_network: printer.is_network,
                    is_virtual: printer.is_virtual,
                    papers,
                    trays,
                    media_types,
                },
            })
        }
        ClientMessage::GetPrintQueue { request_id } => {
            let jobs = state
                .queue
                .lock()
                .await
                .pending_jobs()
                .into_iter()
                .map(|queued| PrintQueueJobInfo {
                    request_id: queued.request_id,
                    batch_id: queued.batch_id,
                    job_id: queued.job.job_id,
                    status: JobStatus::Queued,
                    message: Some("queued".to_string()),
                })
                .collect();

            ClientTextOutcome::response(ServerMessage::PrintQueue { request_id, jobs })
        }
        ClientMessage::Print { request_id, job } => {
            let max_file_size_mb = state.config.read().await.limits.max_file_size_mb;
            if let Err(error) = job.validate_for_acceptance(max_file_size_mb) {
                return ClientTextOutcome::response(job_validation_error_response(
                    Some(request_id),
                    error,
                ));
            }
            let html_local_origin = match html_local_origin(std::slice::from_ref(&job), origin) {
                Ok(origin) => origin,
                Err(message) => {
                    return ClientTextOutcome::response(invalid_message_response(
                        Some(request_id),
                        message,
                    ))
                }
            };

            let job_id = job.job_id.clone();
            let result = state.queue.lock().await.accept_job_with_html_local_origin(
                request_id.clone(),
                job,
                html_local_origin,
            );
            match result {
                Ok(()) => {
                    state.queue_notify.notify_one();
                    ClientTextOutcome {
                        accepted_job_ids: vec![job_id.clone()],
                        response: ServerMessage::JobStatus {
                            request_id: Some(request_id),
                            job_id,
                            status: JobStatus::Queued,
                            message: Some("queued".to_string()),
                        },
                    }
                }
                Err(error) => {
                    ClientTextOutcome::response(queue_error_response(Some(request_id), error))
                }
            }
        }
        ClientMessage::PrintBatch {
            request_id,
            batch_id,
            jobs,
        } => {
            let max_batch_jobs = state.config.read().await.limits.max_batch_jobs;
            if jobs.len() > max_batch_jobs {
                return ClientTextOutcome::response(ServerMessage::Error {
                    request_id: Some(request_id),
                    error_code: ErrorCode::BatchTooLarge,
                    message: "batch contains too many jobs".to_string(),
                });
            }
            let max_file_size_mb = state.config.read().await.limits.max_file_size_mb;
            for job in &jobs {
                if let Err(error) = job.validate_for_acceptance(max_file_size_mb) {
                    return ClientTextOutcome::response(job_validation_error_response(
                        Some(request_id),
                        error,
                    ));
                }
            }
            let html_local_origin = match html_local_origin(&jobs, origin) {
                Ok(origin) => origin,
                Err(message) => {
                    return ClientTextOutcome::response(invalid_message_response(
                        Some(request_id),
                        message,
                    ))
                }
            };

            // Store all accepted task IDs so subsequent worker status broadcasts
            // are filtered back exclusively to this WebSocket connection.
            let queued = jobs.len();
            let job_ids = jobs
                .iter()
                .map(|job| job.job_id.clone())
                .collect::<Vec<_>>();
            let response_job_id = batch_id.clone();
            let result = state
                .queue
                .lock()
                .await
                .accept_batch_with_html_local_origin(
                    request_id.clone(),
                    batch_id,
                    jobs,
                    html_local_origin,
                );
            match result {
                Ok(()) => {
                    state.queue_notify.notify_one();
                    ClientTextOutcome {
                        accepted_job_ids: job_ids,
                        response: ServerMessage::JobStatus {
                            request_id: Some(request_id),
                            job_id: response_job_id,
                            status: JobStatus::Queued,
                            message: Some(format!("batch accepted: {queued} jobs queued")),
                        },
                    }
                }
                Err(error) => {
                    ClientTextOutcome::response(queue_error_response(Some(request_id), error))
                }
            }
        }
    }
}

/// Return allowable local HTML Origin; only accepts address strictly same-origin with WebSocket page.
fn html_local_origin(
    jobs: &[PrintJobInput],
    origin: Option<&str>,
) -> Result<Option<String>, String> {
    let loopback_urls = jobs
        .iter()
        .filter(|job| job.format == SupportedFormat::Html)
        .filter_map(|job| job.file_url.as_deref())
        .filter_map(|file_url| validate_html_file_url(file_url).ok())
        .filter(is_exact_loopback_url)
        .collect::<Vec<_>>();
    if loopback_urls.is_empty() {
        return Ok(None);
    }

    let Some(origin) = origin else {
        return Err("loopback HTML URLs require a matching loopback WebSocket Origin".to_string());
    };
    let origin = url::Url::parse(origin)
        .ok()
        .filter(is_exact_loopback_url)
        .ok_or_else(|| {
            "loopback HTML URLs require a matching loopback WebSocket Origin".to_string()
        })?;
    if loopback_urls
        .iter()
        .any(|file_url| file_url.origin() != origin.origin())
    {
        return Err("loopback HTML URL must match the WebSocket Origin exactly".to_string());
    }

    Ok(Some(origin.origin().ascii_serialization()))
}

/// Determine whether URL uses supported exact loopback address.
fn is_exact_loopback_url(url: &url::Url) -> bool {
    matches!(url.host_str(), Some("127.0.0.1") | Some("::1"))
}

/// Convert task log entry to WebSocket status message for an individual connection.
fn status_message_for_connection(
    entry: &TaskLogEntry,
    accepted_job_ids: &HashSet<String>,
) -> Option<ServerMessage> {
    let job_id = entry.job_id.as_ref()?;
    if !accepted_job_ids.contains(job_id) {
        return None;
    }

    Some(ServerMessage::JobStatus {
        request_id: entry.request_id.clone(),
        job_id: job_id.clone(),
        status: entry.status,
        message: Some(entry.message.clone()),
    })
}

/// Map queue admission failure to protocol error message.
fn queue_error_response(request_id: Option<String>, error: QueueError) -> ServerMessage {
    let error_code = match error {
        QueueError::DuplicateJobId => ErrorCode::JobDuplicated,
        QueueError::DuplicateBatchId => ErrorCode::BatchDuplicated,
        QueueError::InvalidMessage => ErrorCode::InvalidMessage,
    };

    ServerMessage::Error {
        request_id,
        error_code,
        message: error.to_string(),
    }
}

/// Construct error response for invalid protocol fields.
fn invalid_message_response(request_id: Option<String>, message: String) -> ServerMessage {
    ServerMessage::Error {
        request_id,
        error_code: ErrorCode::InvalidMessage,
        message,
    }
}

/// Map task field validation failure to WebSocket protocol error message.
fn job_validation_error_response(
    request_id: Option<String>,
    error: JobValidationError,
) -> ServerMessage {
    let error_code = match error {
        JobValidationError::FileTooLarge => ErrorCode::FileTooLarge,
        JobValidationError::MissingRawData
        | JobValidationError::RawFileUrlNotAllowed
        | JobValidationError::RawPaperNotAllowed
        | JobValidationError::RawCopiesNotAllowed
        | JobValidationError::MissingFileUrl
        | JobValidationError::FileRawDataNotAllowed
        | JobValidationError::InvalidRawData
        | JobValidationError::MissingHtmlFileUrl
        | JobValidationError::InvalidHtmlFileUrl
        | JobValidationError::HtmlInlineNotAllowed
        | JobValidationError::MissingRawHtml
        | JobValidationError::RawHtmlFileUrlNotAllowed
        | JobValidationError::HtmlDataBase64NotAllowed
        | JobValidationError::NonHtmlHtmlNotAllowed
        | JobValidationError::NonHtmlWaitNotAllowed
        | JobValidationError::HtmlWaitOutOfRange => ErrorCode::InvalidMessage,
    };

    ServerMessage::Error {
        request_id,
        error_code,
        message: error.to_string(),
    }
}

/// Map print backend error to WebSocket protocol error message.
fn print_error_response(request_id: Option<String>, error: PrintError) -> ServerMessage {
    let error_code = match error {
        PrintError::PrinterNotFound(_) => ErrorCode::PrinterNotFound,
        PrintError::PaperNotFound(_) => ErrorCode::PaperNotFound,
        PrintError::UnsupportedPlatform | PrintError::CommandFailed { .. } => {
            ErrorCode::PrintFailed
        }
    };

    ServerMessage::Error {
        request_id,
        error_code,
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config::{AgentConfig, SecurityConfig, ServiceConfig},
        logs::TaskLogEntry,
        printing::{
            PaperInfo, PrintBackend, PrintOptions, PrintResult, PrintSubmission, PrinterInfo,
            RawPrintOptions,
        },
        protocol::{ErrorCode, JobStatus, JobValidationError, ServerMessage},
        queue::QueueError,
        server::{configured_addr, is_client_ip_allowed_for_state, is_ws_origin_allowed},
        state::AgentState,
    };
    use axum::{
        body::Body,
        extract::connect_info::ConnectInfo,
        http::{Method, Request, StatusCode},
    };
    use std::{
        collections::HashSet,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        path::Path,
    };
    use tower::ServiceExt;

    #[test]
    fn router_builds_with_state() {
        let _router = super::router(AgentState::new(AgentConfig::default()));
    }

    #[test]
    fn html_source_validation_errors_map_to_invalid_message() {
        assert!(matches!(
            super::job_validation_error_response(
                Some("request-1".to_string()),
                JobValidationError::InvalidHtmlFileUrl,
            ),
            ServerMessage::Error {
                error_code: ErrorCode::InvalidMessage,
                ..
            }
        ));
        assert!(matches!(
            super::queue_error_response(Some("request-1".to_string()), QueueError::InvalidMessage),
            ServerMessage::Error {
                error_code: ErrorCode::InvalidMessage,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn rest_routes_are_not_exposed() {
        for path in ["/health", "/printers", "/config", "/logs", "/print/test"] {
            let response = super::router(AgentState::new(AgentConfig::default()))
                .oneshot(
                    Request::builder()
                        .method(Method::GET)
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }
    }

    #[test]
    fn configured_addr_binds_all_interfaces_and_uses_configured_port() {
        let config = AgentConfig {
            service: ServiceConfig {
                host: "127.0.0.1".to_string(),
                port: 19001,
            },
            ..AgentConfig::default()
        };

        let addr = configured_addr(&config).unwrap();

        assert_eq!(addr.to_string(), "0.0.0.0:19001");
    }

    #[test]
    fn status_event_for_connection_filters_unaccepted_jobs() {
        let mut accepted = HashSet::new();
        accepted.insert("job-1".to_string());
        let matching = TaskLogEntry {
            timestamp: "2026-07-04T00:00:00Z".to_string(),
            request_id: Some("request-1".to_string()),
            batch_id: None,
            job_id: Some("job-1".to_string()),
            origin: None,
            status: JobStatus::Printing,
            message: "printing".to_string(),
        };
        let other = TaskLogEntry {
            job_id: Some("job-2".to_string()),
            ..matching.clone()
        };

        assert_eq!(
            super::status_message_for_connection(&matching, &accepted),
            Some(ServerMessage::JobStatus {
                request_id: Some("request-1".to_string()),
                job_id: "job-1".to_string(),
                status: JobStatus::Printing,
                message: Some("printing".to_string()),
            })
        );
        assert_eq!(
            super::status_message_for_connection(&other, &accepted),
            None
        );
    }

    #[test]
    fn job_status_message_serializes_submitted_status() {
        let json = serde_json::to_string(&ServerMessage::JobStatus {
            request_id: Some("request-1".to_string()),
            job_id: "job-1".to_string(),
            status: JobStatus::Submitted,
            message: Some("submitted to system print queue".to_string()),
        })
        .unwrap();

        assert!(json.contains(r#""status":"submitted""#));
        assert!(!json.contains(r#""status":"success""#));
    }

    #[tokio::test]
    async fn websocket_get_printers_list_marks_configured_default_printer() {
        let mut config = AgentConfig::default();
        config.printing.default_printer = Some("Zebra ZD421".to_string());
        let state = AgentState::with_printing(
            config,
            Box::new(ListingPrintBackend {
                printers: vec![
                    PrinterInfo {
                        name: "Windows system default".to_string(),
                        is_default: true,
                        dpi: None,
                        port: None,
                        is_local: None,
                        is_network: None,
                        is_virtual: None,
                    },
                    PrinterInfo {
                        name: "Zebra ZD421".to_string(),
                        is_default: false,
                        dpi: Some(203),
                        port: Some("usb://Zebra/ZD421".to_string()),
                        is_local: Some(true),
                        is_network: Some(false),
                        is_virtual: Some(false),
                    },
                ],
                papers: vec![],
                trays: vec![],
                media_types: vec![],
            }),
        );

        let outcome = super::handle_client_text(
            &state,
            r#"{"type":"get_printers_list","request_id":"REQ-PRINTERS"}"#,
            None,
        )
        .await;

        assert_eq!(
            outcome.response,
            ServerMessage::PrintersList {
                request_id: "REQ-PRINTERS".to_string(),
                printers: vec![
                    PrinterInfo {
                        name: "Windows system default".to_string(),
                        is_default: false,
                        dpi: None,
                        port: None,
                        is_local: None,
                        is_network: None,
                        is_virtual: None,
                    },
                    PrinterInfo {
                        name: "Zebra ZD421".to_string(),
                        is_default: true,
                        dpi: Some(203),
                        port: Some("usb://Zebra/ZD421".to_string()),
                        is_local: Some(true),
                        is_network: Some(false),
                        is_virtual: Some(false),
                    },
                ],
            }
        );
    }

    #[tokio::test]
    async fn websocket_get_printer_info_returns_backend_papers() {
        let state = AgentState::with_printing(
            AgentConfig::default(),
            Box::new(ListingPrintBackend {
                printers: vec![PrinterInfo {
                    name: "Zebra ZD421".to_string(),
                    is_default: true,
                    dpi: Some(203),
                    port: Some("usb://Zebra/ZD421".to_string()),
                    is_local: Some(true),
                    is_network: Some(false),
                    is_virtual: Some(false),
                }],
                papers: vec![PaperInfo {
                    id: "label_60x40".to_string(),
                    name: "60 x 40 mm".to_string(),
                    width_mm: 60.0,
                    height_mm: 40.0,
                }],
                trays: vec![crate::printing::PrinterTrayInfo {
                    id: "tray-1".to_string(),
                    name: "Tray 1".to_string(),
                }],
                media_types: vec![crate::printing::PrinterMediaTypeInfo {
                    id: "thermal-label".to_string(),
                    name: "Thermal Label".to_string(),
                }],
            }),
        );

        let outcome = super::handle_client_text(
            &state,
            r#"{"type":"get_printer_info","request_id":"REQ-INFO","printer_name":"Zebra ZD421"}"#,
            None,
        )
        .await;

        assert_eq!(
            outcome.response,
            ServerMessage::PrinterInfo {
                request_id: "REQ-INFO".to_string(),
                printer: super::PrinterDetails {
                    name: "Zebra ZD421".to_string(),
                    is_default: true,
                    dpi: Some(203),
                    port: Some("usb://Zebra/ZD421".to_string()),
                    is_local: Some(true),
                    is_network: Some(false),
                    is_virtual: Some(false),
                    papers: vec![PaperInfo {
                        id: "label_60x40".to_string(),
                        name: "60 x 40 mm".to_string(),
                        width_mm: 60.0,
                        height_mm: 40.0,
                    }],
                    trays: vec![crate::printing::PrinterTrayInfo {
                        id: "tray-1".to_string(),
                        name: "Tray 1".to_string(),
                    }],
                    media_types: vec![crate::printing::PrinterMediaTypeInfo {
                        id: "thermal-label".to_string(),
                        name: "Thermal Label".to_string(),
                    }],
                },
            }
        );
    }

    #[tokio::test]
    async fn websocket_get_print_queue_returns_pending_jobs() {
        let state = AgentState::new(AgentConfig::default());
        state
            .queue
            .lock()
            .await
            .accept_job(
                "REQ-QUEUE-ITEM".to_string(),
                crate::protocol::PrintJobInput {
                    job_id: "JOB-QUEUE-ITEM".to_string(),
                    format: crate::protocol::SupportedFormat::Pdf,
                    printer_name: None,
                    file_url: Some("https://example.com/label.pdf".to_string()),
                    data_base64: None,
                    html: None,
                    wait_ms: None,
                    copies: Some(1),
                    paper: None,
                },
            )
            .unwrap();

        let outcome = super::handle_client_text(
            &state,
            r#"{"type":"get_print_queue","request_id":"REQ-QUEUE"}"#,
            None,
        )
        .await;

        assert_eq!(
            outcome.response,
            ServerMessage::PrintQueue {
                request_id: "REQ-QUEUE".to_string(),
                jobs: vec![super::PrintQueueJobInfo {
                    request_id: "REQ-QUEUE-ITEM".to_string(),
                    batch_id: None,
                    job_id: "JOB-QUEUE-ITEM".to_string(),
                    status: JobStatus::Queued,
                    message: Some("queued".to_string()),
                }],
            }
        );
    }

    #[tokio::test]
    async fn websocket_allows_html_from_its_exact_loopback_origin() {
        let state = AgentState::new(AgentConfig::default());
        let outcome = super::handle_client_text(
            &state,
            r#"{"type":"print","request_id":"REQ-LOCAL","job_id":"JOB-LOCAL","format":"html","file_url":"http://127.0.0.1:6688/static/Sample.html","copies":1}"#,
            Some("http://127.0.0.1:6688"),
        )
        .await;

        assert!(matches!(
            outcome.response,
            ServerMessage::JobStatus {
                status: JobStatus::Queued,
                ..
            }
        ));
        let queued = state.queue.lock().await.pending_jobs();
        assert_eq!(
            queued[0].html_local_origin.as_deref(),
            Some("http://127.0.0.1:6688")
        );
    }

    #[tokio::test]
    async fn websocket_rejects_html_from_a_different_loopback_origin() {
        let state = AgentState::new(AgentConfig::default());
        let outcome = super::handle_client_text(
            &state,
            r#"{"type":"print","request_id":"REQ-LOCAL","job_id":"JOB-LOCAL","format":"html","file_url":"http://127.0.0.1:6688/static/Sample.html","copies":1}"#,
            Some("http://127.0.0.1:5173"),
        )
        .await;

        assert!(matches!(
            outcome.response,
            ServerMessage::Error {
                error_code: ErrorCode::InvalidMessage,
                ..
            }
        ));
        assert!(state.queue.lock().await.pending_jobs().is_empty());
    }

    #[tokio::test]
    async fn websocket_batch_does_not_grant_loopback_access_to_public_html() {
        let state = AgentState::new(AgentConfig::default());
        let outcome = super::handle_client_text(
            &state,
            r#"{"type":"print_batch","request_id":"REQ-BATCH","batch_id":"BATCH-LOCAL","jobs":[{"job_id":"JOB-LOCAL","format":"html","file_url":"http://127.0.0.1:6688/static/Sample.html","copies":1},{"job_id":"JOB-PUBLIC","format":"html","file_url":"https://example.com/Sample.html","copies":1}]}"#,
            Some("http://127.0.0.1:6688"),
        )
        .await;

        assert!(matches!(
            outcome.response,
            ServerMessage::JobStatus {
                status: JobStatus::Queued,
                ..
            }
        ));
        let queued = state.queue.lock().await.pending_jobs();
        assert_eq!(
            queued[0].html_local_origin.as_deref(),
            Some("http://127.0.0.1:6688")
        );
        assert_eq!(queued[1].html_local_origin, None);
    }

    struct ListingPrintBackend {
        printers: Vec<PrinterInfo>,
        papers: Vec<PaperInfo>,
        trays: Vec<crate::printing::PrinterTrayInfo>,
        media_types: Vec<crate::printing::PrinterMediaTypeInfo>,
    }

    impl PrintBackend for ListingPrintBackend {
        fn list_printers(&self) -> PrintResult<Vec<PrinterInfo>> {
            Ok(self.printers.clone())
        }

        fn list_papers(&self, _printer_name: &str) -> PrintResult<Vec<PaperInfo>> {
            Ok(self.papers.clone())
        }

        fn list_trays(
            &self,
            _printer_name: &str,
        ) -> PrintResult<Vec<crate::printing::PrinterTrayInfo>> {
            Ok(self.trays.clone())
        }

        fn list_media_types(
            &self,
            _printer_name: &str,
        ) -> PrintResult<Vec<crate::printing::PrinterMediaTypeInfo>> {
            Ok(self.media_types.clone())
        }

        fn print_pdf(&self, _path: &Path, _options: &PrintOptions) -> PrintResult<PrintSubmission> {
            Ok(mock_submission())
        }

        fn print_raw(
            &self,
            _data: &[u8],
            _options: &RawPrintOptions,
        ) -> PrintResult<PrintSubmission> {
            Ok(mock_submission())
        }
    }

    fn mock_submission() -> PrintSubmission {
        PrintSubmission {
            submitted_at: "2026-07-06T00:00:00Z".to_string(),
            backend: "mock".to_string(),
            system_job_id: None,
            tracking_supported: false,
        }
    }

    #[tokio::test]
    async fn ws_origin_gate_uses_configured_allowed_origins() {
        let config = AgentConfig {
            security: SecurityConfig {
                allowed_origins: vec!["http://localhost:5173".to_string()],
                allowed_ips: vec!["127.0.0.1".to_string()],
            },
            ..AgentConfig::default()
        };
        let state = AgentState::new(config);

        assert!(is_ws_origin_allowed(&state, Some("http://localhost:5173")).await);
        assert!(!is_ws_origin_allowed(&state, Some("https://evil.example")).await);
        assert!(!is_ws_origin_allowed(&state, None).await);
    }

    #[tokio::test]
    async fn client_ip_gate_allows_loopback_even_when_missing_from_config() {
        let config = AgentConfig {
            security: SecurityConfig {
                allowed_origins: Vec::new(),
                allowed_ips: Vec::new(),
            },
            ..AgentConfig::default()
        };
        let state = AgentState::new(config);

        assert!(
            is_client_ip_allowed_for_state(&state, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))).await
        );
    }

    #[tokio::test]
    async fn client_ip_gate_uses_single_ip_and_cidr_entries() {
        let config = AgentConfig {
            security: SecurityConfig {
                allowed_origins: Vec::new(),
                allowed_ips: vec![
                    "127.0.0.1".to_string(),
                    "192.168.1.0/24".to_string(),
                    "10.0.0.8".to_string(),
                ],
            },
            ..AgentConfig::default()
        };
        let state = AgentState::new(config);

        assert!(
            is_client_ip_allowed_for_state(&state, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)))
                .await
        );
        assert!(
            is_client_ip_allowed_for_state(&state, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 8))).await
        );
        assert!(
            !is_client_ip_allowed_for_state(&state, IpAddr::V4(Ipv4Addr::new(192, 168, 2, 20)))
                .await
        );
    }

    #[tokio::test]
    async fn websocket_route_rejects_disallowed_client_ip() {
        let config = AgentConfig {
            security: SecurityConfig {
                allowed_origins: Vec::new(),
                allowed_ips: vec!["127.0.0.1".to_string()],
            },
            ..AgentConfig::default()
        };
        let app = super::router(AgentState::new(config));
        let mut request = Request::builder()
            .method(Method::GET)
            .uri("/ws")
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([192, 168, 1, 20], 50000))));

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
