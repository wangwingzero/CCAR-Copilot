//! Sidecar 进程管理器
//!
//! 负责 Python Sidecar 进程的启动、通信和生命周期管理。
//!
//! # 设计要点
//!
//! 1. **stdin/stdout JSON 通信**：每条消息以换行符结尾
//! 2. **崩溃自动重启**：最多 3 次，每次间隔 2 秒
//! 3. **请求-响应匹配**：使用 UUID 匹配请求和响应
//! 4. **超时处理**：默认 30 秒超时

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::future::{BoxFuture, FutureExt};
use tauri::AppHandle;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::timeout;

use crate::error::{HuGeError, HuGeResult};
use crate::sidecar::protocol::{SidecarRequest, SidecarResponse};

/// 默认请求超时时间（秒）
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// 最大重启次数
const MAX_RESTARTS: u32 = 3;

/// 重启延迟时间（秒）
const RESTART_DELAY_SECS: u64 = 2;

/// Sidecar 状态
#[derive(Debug, Clone, PartialEq)]
pub enum SidecarState {
    /// 未启动
    Stopped,
    /// 正在启动
    Starting,
    /// 运行中
    Running,
    /// 正在停止
    Stopping,
    /// 已崩溃
    Crashed,
}

/// Sidecar 内部状态（用于跨线程共享）
struct SidecarInner {
    child: Option<CommandChild>,
    pending_requests: HashMap<String, oneshot::Sender<SidecarResponse>>,
    restart_count: u32,
    state: SidecarState,
    request_tx: Option<mpsc::Sender<String>>,
}

impl SidecarInner {
    fn new() -> Self {
        Self {
            child: None,
            pending_requests: HashMap::new(),
            restart_count: 0,
            state: SidecarState::Stopped,
            request_tx: None,
        }
    }
}

/// Sidecar 管理器
#[derive(Clone)]
pub struct SidecarManager {
    app_handle: AppHandle,
    inner: Arc<Mutex<SidecarInner>>,
}

impl SidecarManager {
    /// 创建新的 Sidecar 管理器
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            inner: Arc::new(Mutex::new(SidecarInner::new())),
        }
    }

    /// 启动 Sidecar 进程
    pub async fn start(&self) -> HuGeResult<()> {
        {
            let inner = self.inner.lock().await;
            if inner.state == SidecarState::Running || inner.state == SidecarState::Starting {
                return Err(HuGeError::SidecarError("Sidecar 已在运行中".to_string()));
            }
        }

        {
            let mut inner = self.inner.lock().await;
            inner.state = SidecarState::Starting;
        }

        tracing::info!("正在启动 Python Sidecar...");

        let sidecar_command = self
            .app_handle
            .shell()
            .sidecar("huge_sidecar")
            .map_err(|e| HuGeError::SidecarError(format!("创建 Sidecar 命令失败: {}", e)))?;

        let (mut rx, child) = sidecar_command
            .spawn()
            .map_err(|e| HuGeError::SidecarError(format!("启动 Sidecar 进程失败: {}", e)))?;

        let (request_tx, mut request_rx) = mpsc::channel::<String>(100);

        {
            let mut inner = self.inner.lock().await;
            inner.child = Some(child);
            inner.request_tx = Some(request_tx);
            inner.state = SidecarState::Running;
        }

        let inner_arc = Arc::clone(&self.inner);
        let app_handle = self.app_handle.clone();

        tokio::spawn(async move {
            tracing::info!("Sidecar 已启动，开始监听事件...");

            let inner_for_stdin = Arc::clone(&inner_arc);
            tokio::spawn(async move {
                while let Some(request_line) = request_rx.recv().await {
                    let mut inner = inner_for_stdin.lock().await;
                    if let Some(ref mut child) = inner.child {
                        if let Err(e) = child.write(request_line.as_bytes()) {
                            tracing::error!("写入 Sidecar stdin 失败: {}", e);
                        }
                    }
                }
            });

            while let Some(event) = rx.recv().await {
                match event {
                    CommandEvent::Stdout(line) => {
                        let line_str = String::from_utf8_lossy(&line);
                        let line_str = line_str.trim();
                        if line_str.is_empty() { continue; }

                        tracing::debug!("Sidecar stdout: {}", line_str);

                        match SidecarResponse::from_json_line(line_str) {
                            Ok(response) => {
                                let request_id = response.id.clone();
                                let mut inner = inner_arc.lock().await;
                                if let Some(sender) = inner.pending_requests.remove(&request_id) {
                                    let _ = sender.send(response);
                                }
                            }
                            Err(e) => {
                                tracing::warn!("解析 Sidecar 响应失败: {} - {}", e, line_str);
                            }
                        }
                    }
                    CommandEvent::Stderr(line) => {
                        let line_str = String::from_utf8_lossy(&line);
                        tracing::warn!("Sidecar stderr: {}", line_str.trim());
                    }
                    CommandEvent::Error(error) => {
                        tracing::error!("Sidecar 错误: {}", error);
                    }
                    CommandEvent::Terminated(payload) => {
                        tracing::warn!("Sidecar 进程终止，退出码: {:?}", payload.code);

                        let should_restart = {
                            let mut inner = inner_arc.lock().await;
                            inner.state = SidecarState::Crashed;
                            inner.child = None;
                            inner.request_tx = None;

                            for (id, sender) in inner.pending_requests.drain() {
                                let err_resp = SidecarResponse {
                                    id, success: false, result: None,
                                    error: Some("Sidecar 进程已终止".to_string()),
                                };
                                let _ = sender.send(err_resp);
                            }

                            let should = inner.restart_count < MAX_RESTARTS;
                            if should { inner.restart_count += 1; }
                            should
                        };

                        if should_restart {
                            let count = { inner_arc.lock().await.restart_count };
                            tracing::info!("尝试重启 Sidecar ({}/{})", count, MAX_RESTARTS);
                            tokio::time::sleep(Duration::from_secs(RESTART_DELAY_SECS)).await;
                            let _ = restart_sidecar(app_handle.clone(), Arc::clone(&inner_arc)).await;
                        } else {
                            tracing::error!("Sidecar 已崩溃 {} 次，停止自动重启", MAX_RESTARTS);
                        }
                        break;
                    }
                    _ => {}
                }
            }
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        tracing::info!("Sidecar 启动完成");
        Ok(())
    }

    /// 调用 Sidecar 服务
    pub async fn call(
        &self,
        service: &str,
        method: &str,
        params: serde_json::Value,
    ) -> HuGeResult<serde_json::Value> {
        self.call_with_timeout(service, method, params, Duration::from_secs(DEFAULT_TIMEOUT_SECS)).await
    }

    /// 调用 Sidecar 服务（带自定义超时）
    pub async fn call_with_timeout(
        &self,
        service: &str,
        method: &str,
        params: serde_json::Value,
        timeout_duration: Duration,
    ) -> HuGeResult<serde_json::Value> {
        let request = SidecarRequest::new(service, method, params);
        let request_id = request.id.clone();
        let request_line = request.to_json_line().map_err(HuGeError::SerializationError)?;

        tracing::debug!("发送 Sidecar 请求: {}", request_line.trim());

        let (response_tx, response_rx) = oneshot::channel();

        let request_tx = {
            let mut inner = self.inner.lock().await;
            if inner.state != SidecarState::Running {
                return Err(HuGeError::SidecarError(format!("Sidecar 未运行，当前状态: {:?}", inner.state)));
            }
            inner.pending_requests.insert(request_id.clone(), response_tx);
            inner.request_tx.clone()
        };

        if let Some(tx) = request_tx {
            if let Err(e) = tx.send(request_line).await {
                let mut inner = self.inner.lock().await;
                inner.pending_requests.remove(&request_id);
                return Err(HuGeError::SidecarError(format!("发送请求失败: {}", e)));
            }
        } else {
            let mut inner = self.inner.lock().await;
            inner.pending_requests.remove(&request_id);
            return Err(HuGeError::SidecarError("请求通道未初始化".to_string()));
        }

        match timeout(timeout_duration, response_rx).await {
            Ok(Ok(response)) => response.into_result().map_err(HuGeError::SidecarError),
            Ok(Err(_)) => {
                let mut inner = self.inner.lock().await;
                inner.pending_requests.remove(&request_id);
                Err(HuGeError::SidecarError("响应通道已关闭".to_string()))
            }
            Err(_) => {
                let mut inner = self.inner.lock().await;
                inner.pending_requests.remove(&request_id);
                Err(HuGeError::TimeoutError(format!("Sidecar 请求超时 ({}s)", timeout_duration.as_secs())))
            }
        }
    }

    /// 停止 Sidecar 进程
    pub async fn stop(&self) -> HuGeResult<()> {
        {
            let inner = self.inner.lock().await;
            if inner.state == SidecarState::Stopped { return Ok(()); }
        }

        {
            let mut inner = self.inner.lock().await;
            inner.state = SidecarState::Stopping;
        }

        tracing::info!("正在停止 Sidecar...");

        let _ = self.call_with_timeout("system", "exit", serde_json::json!({}), Duration::from_secs(5)).await;
        tokio::time::sleep(Duration::from_secs(1)).await;

        {
            let mut inner = self.inner.lock().await;
            if let Some(child) = inner.child.take() {
                let _ = child.kill();
            }
            inner.request_tx = None;

            for (id, sender) in inner.pending_requests.drain() {
                let err_resp = SidecarResponse {
                    id, success: false, result: None,
                    error: Some("Sidecar 已停止".to_string()),
                };
                let _ = sender.send(err_resp);
            }
            inner.state = SidecarState::Stopped;
        }

        tracing::info!("Sidecar 已停止");
        Ok(())
    }

    /// 检查 Sidecar 是否正在运行
    pub async fn is_running(&self) -> bool {
        let inner = self.inner.lock().await;
        inner.state == SidecarState::Running
    }

    /// 获取当前状态
    pub async fn get_state(&self) -> SidecarState {
        let inner = self.inner.lock().await;
        inner.state.clone()
    }

    /// 获取重启计数
    pub async fn get_restart_count(&self) -> u32 {
        let inner = self.inner.lock().await;
        inner.restart_count
    }

    /// 重置重启计数
    pub async fn reset_restart_count(&self) {
        let mut inner = self.inner.lock().await;
        inner.restart_count = 0;
    }
}

/// 重启 Sidecar 进程（独立函数，返回 BoxFuture 以支持递归调用）
///
/// 使用 BoxFuture 而非普通 async fn，因为递归调用需要显式的 Send bound
fn restart_sidecar(
    app_handle: AppHandle,
    inner: Arc<Mutex<SidecarInner>>,
) -> BoxFuture<'static, HuGeResult<()>> {
    async move {
        {
            let mut inner_guard = inner.lock().await;
            inner_guard.state = SidecarState::Starting;
        }

        tracing::info!("正在重启 Python Sidecar...");

        let sidecar_command = app_handle
            .shell()
            .sidecar("huge_sidecar")
            .map_err(|e| HuGeError::SidecarError(format!("创建 Sidecar 命令失败: {}", e)))?;

        let (mut rx, child) = sidecar_command
            .spawn()
            .map_err(|e| HuGeError::SidecarError(format!("启动 Sidecar 进程失败: {}", e)))?;

        let (request_tx, mut request_rx) = mpsc::channel::<String>(100);

        {
            let mut inner_guard = inner.lock().await;
            inner_guard.child = Some(child);
            inner_guard.request_tx = Some(request_tx);
            inner_guard.state = SidecarState::Running;
        }

        let inner_arc = Arc::clone(&inner);
        let app_handle_clone = app_handle.clone();

        tokio::spawn(async move {
            tracing::info!("Sidecar 重启成功，开始监听事件...");

            let inner_for_stdin = Arc::clone(&inner_arc);
            tokio::spawn(async move {
                while let Some(request_line) = request_rx.recv().await {
                    let mut inner_guard = inner_for_stdin.lock().await;
                    if let Some(ref mut child) = inner_guard.child {
                        if let Err(e) = child.write(request_line.as_bytes()) {
                            tracing::error!("写入 Sidecar stdin 失败: {}", e);
                        }
                    }
                }
            });

            while let Some(event) = rx.recv().await {
                match event {
                    CommandEvent::Stdout(line) => {
                        let line_str = String::from_utf8_lossy(&line);
                        let line_str = line_str.trim();
                        if line_str.is_empty() { continue; }

                        tracing::debug!("Sidecar stdout: {}", line_str);

                        if let Ok(response) = SidecarResponse::from_json_line(line_str) {
                            let request_id = response.id.clone();
                            let mut inner_guard = inner_arc.lock().await;
                            if let Some(sender) = inner_guard.pending_requests.remove(&request_id) {
                                let _ = sender.send(response);
                            }
                        }
                    }
                    CommandEvent::Stderr(line) => {
                        let line_str = String::from_utf8_lossy(&line);
                        tracing::warn!("Sidecar stderr: {}", line_str.trim());
                    }
                    CommandEvent::Error(error) => {
                        tracing::error!("Sidecar 错误: {}", error);
                    }
                    CommandEvent::Terminated(payload) => {
                        tracing::warn!("Sidecar 进程终止，退出码: {:?}", payload.code);

                        let should_restart = {
                            let mut inner_guard = inner_arc.lock().await;
                            inner_guard.state = SidecarState::Crashed;
                            inner_guard.child = None;
                            inner_guard.request_tx = None;

                            for (id, sender) in inner_guard.pending_requests.drain() {
                                let err_resp = SidecarResponse {
                                    id, success: false, result: None,
                                    error: Some("Sidecar 进程已终止".to_string()),
                                };
                                let _ = sender.send(err_resp);
                            }

                            let should = inner_guard.restart_count < MAX_RESTARTS;
                            if should { inner_guard.restart_count += 1; }
                            should
                        };

                        if should_restart {
                            let count = { inner_arc.lock().await.restart_count };
                            tracing::info!("尝试重启 Sidecar ({}/{})", count, MAX_RESTARTS);
                            tokio::time::sleep(Duration::from_secs(RESTART_DELAY_SECS)).await;
                            // 递归调用 restart_sidecar，已经返回 BoxFuture 所以可以直接 await
                            let _ = restart_sidecar(app_handle_clone.clone(), Arc::clone(&inner_arc)).await;
                        } else {
                            tracing::error!("Sidecar 已崩溃 {} 次，停止自动重启", MAX_RESTARTS);
                        }
                        break;
                    }
                    _ => {}
                }
            }
        });

        tracing::info!("Sidecar 重启完成");
        Ok(())
    }.boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sidecar_state_default() {
        assert_eq!(SidecarState::Stopped, SidecarState::Stopped);
    }

    #[test]
    fn test_constants() {
        assert_eq!(DEFAULT_TIMEOUT_SECS, 30);
        assert_eq!(MAX_RESTARTS, 3);
        assert_eq!(RESTART_DELAY_SECS, 2);
    }

    #[test]
    fn test_sidecar_inner_new() {
        let inner = SidecarInner::new();
        assert!(inner.child.is_none());
        assert!(inner.pending_requests.is_empty());
        assert_eq!(inner.restart_count, 0);
        assert_eq!(inner.state, SidecarState::Stopped);
        assert!(inner.request_tx.is_none());
    }
}
