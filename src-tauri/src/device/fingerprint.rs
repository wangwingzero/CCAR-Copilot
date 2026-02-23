//! 设备指纹生成
//!
//! 使用多个硬件标识符组合生成唯一的设备指纹：
//! - SMBIOS UUID（主板相关）
//! - MAC 地址（过滤虚拟网卡）
//! - 启动磁盘序列号
//!
//! 最终使用 SHA-256 哈希生成 64 字符的指纹
//!
//! 性能优化：设备指纹会缓存到本地文件，避免每次启动都调用 PowerShell

use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tracing::{debug, warn};

/// 缓存的设备信息版本号（更改此值会强制重新生成指纹）
const CACHE_VERSION: u32 = 1;

/// 设备信息结构
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// 设备唯一指纹（SHA-256 哈希）
    pub machine_id: String,
    /// 计算机名称
    pub device_name: String,
    /// 操作系统版本
    pub os_version: String,
}

/// 缓存的设备信息（用于序列化）
#[derive(Debug, Serialize, Deserialize)]
struct CachedDeviceInfo {
    version: u32,
    machine_id: String,
    device_name: String,
    os_version: String,
}

/// 获取缓存文件路径
fn get_cache_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|p| p.join("HuGeScreenshot").join("device_fingerprint.json"))
}

/// 从缓存加载设备信息
fn load_from_cache() -> Option<DeviceInfo> {
    let cache_path = get_cache_path()?;
    
    if !cache_path.exists() {
        debug!("设备指纹缓存文件不存在");
        return None;
    }
    
    let content = fs::read_to_string(&cache_path).ok()?;
    let cached: CachedDeviceInfo = serde_json::from_str(&content).ok()?;
    
    // 检查版本号
    if cached.version != CACHE_VERSION {
        debug!("设备指纹缓存版本不匹配，需要重新生成");
        return None;
    }
    
    // 验证 machine_id 格式（64 字符十六进制）
    if cached.machine_id.len() != 64 || !cached.machine_id.chars().all(|c| c.is_ascii_hexdigit()) {
        warn!("设备指纹缓存格式无效");
        return None;
    }
    
    debug!("从缓存加载设备指纹成功");
    Some(DeviceInfo {
        machine_id: cached.machine_id,
        device_name: cached.device_name,
        os_version: cached.os_version,
    })
}

/// 保存设备信息到缓存
fn save_to_cache(info: &DeviceInfo) {
    let Some(cache_path) = get_cache_path() else {
        return;
    };
    
    // 确保目录存在
    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    
    let cached = CachedDeviceInfo {
        version: CACHE_VERSION,
        machine_id: info.machine_id.clone(),
        device_name: info.device_name.clone(),
        os_version: info.os_version.clone(),
    };
    
    match serde_json::to_string_pretty(&cached) {
        Ok(content) => {
            if let Err(e) = fs::write(&cache_path, content) {
                warn!("保存设备指纹缓存失败: {}", e);
            } else {
                debug!("设备指纹已缓存到: {:?}", cache_path);
            }
        }
        Err(e) => {
            warn!("序列化设备指纹失败: {}", e);
        }
    }
}

/// 生成设备唯一标识符
///
/// 组合多个硬件标识符并使用 SHA-256 哈希
///
/// # Returns
/// 64 字符的十六进制字符串
pub fn generate_machine_id() -> String {
    let mut hasher = Sha256::new();

    // 1. 获取 SMBIOS UUID
    if let Some(uuid) = get_smbios_uuid() {
        hasher.update(uuid.as_bytes());
    }

    // 2. 获取 MAC 地址
    if let Some(mac) = get_primary_mac_address() {
        hasher.update(mac.as_bytes());
    }

    // 3. 获取磁盘序列号
    if let Some(serial) = get_boot_disk_serial() {
        hasher.update(serial.as_bytes());
    }

    // 4. 添加计算机名作为附加因素
    if let Some(hostname) = get_hostname() {
        hasher.update(hostname.as_bytes());
    }

    // 生成最终哈希
    let result = hasher.finalize();
    format!("{:x}", result)
}

/// 获取完整的设备信息
///
/// 优先从缓存加载，缓存不存在或无效时重新生成
pub fn get_device_info() -> DeviceInfo {
    // 尝试从缓存加载
    if let Some(cached) = load_from_cache() {
        return cached;
    }
    
    debug!("开始生成设备指纹（首次运行或缓存失效）...");
    
    // 生成新的设备信息
    let info = DeviceInfo {
        machine_id: generate_machine_id(),
        device_name: get_hostname().unwrap_or_else(|| "Unknown".to_string()),
        os_version: get_os_version(),
    };
    
    // 保存到缓存
    save_to_cache(&info);
    
    info
}

/// 获取 SMBIOS UUID
///
/// 使用 PowerShell 获取系统 UUID（比 wmic 更可靠，wmic 已弃用）
fn get_smbios_uuid() -> Option<String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-CimInstance -ClassName Win32_ComputerSystemProduct).UUID",
        ])
        .output()
        .ok()?;

    if output.status.success() {
        let uuid = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        // 过滤无效 UUID（全 0 或全 F）
        if !uuid.is_empty()
            && !uuid.chars().all(|c| c == '0' || c == '-')
            && !uuid.chars().all(|c| c == 'F' || c == 'f' || c == '-')
        {
            return Some(uuid);
        }
    }

    None
}

/// 获取主要 MAC 地址
///
/// 过滤虚拟网卡，优先选择物理网卡
fn get_primary_mac_address() -> Option<String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            r#"Get-NetAdapter -Physical | Where-Object {$_.Status -eq 'Up'} | Select-Object -First 1 -ExpandProperty MacAddress"#,
        ])
        .output()
        .ok()?;

    if output.status.success() {
        let mac = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        if !mac.is_empty() {
            return Some(mac);
        }
    }

    // 降级方案：获取任意物理网卡
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            r#"Get-NetAdapter -Physical | Select-Object -First 1 -ExpandProperty MacAddress"#,
        ])
        .output()
        .ok()?;

    if output.status.success() {
        let mac = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        if !mac.is_empty() {
            return Some(mac);
        }
    }

    None
}

/// 获取启动磁盘序列号
fn get_boot_disk_serial() -> Option<String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            r#"(Get-PhysicalDisk | Where-Object {$_.DeviceId -eq 0} | Select-Object -ExpandProperty SerialNumber) -replace '\s+', ''"#,
        ])
        .output()
        .ok()?;

    if output.status.success() {
        let serial = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        if !serial.is_empty() {
            return Some(serial);
        }
    }

    // 降级方案：获取第一个物理磁盘
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            r#"(Get-PhysicalDisk | Select-Object -First 1 -ExpandProperty SerialNumber) -replace '\s+', ''"#,
        ])
        .output()
        .ok()?;

    if output.status.success() {
        let serial = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        if !serial.is_empty() {
            return Some(serial);
        }
    }

    None
}

/// 获取计算机名
fn get_hostname() -> Option<String> {
    hostname::get()
        .ok()
        .map(|h| h.to_string_lossy().to_string())
}

/// 获取操作系统版本
fn get_os_version() -> String {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            r#"(Get-CimInstance Win32_OperatingSystem).Caption + ' ' + (Get-CimInstance Win32_OperatingSystem).Version"#,
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => format!("Windows {}", std::env::consts::OS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_machine_id() {
        let id1 = generate_machine_id();
        let id2 = generate_machine_id();

        // 同一台机器应该生成相同的指纹
        assert_eq!(id1, id2);

        // 指纹应该是 64 字符的十六进制字符串
        assert_eq!(id1.len(), 64);
        assert!(id1.chars().all(|c| c.is_ascii_hexdigit()));

        println!("Machine ID: {}", id1);
    }

    #[test]
    fn test_get_device_info() {
        let info = get_device_info();

        println!("Device Info:");
        println!("  Machine ID: {}", info.machine_id);
        println!("  Device Name: {}", info.device_name);
        println!("  OS Version: {}", info.os_version);

        assert!(!info.machine_id.is_empty());
        assert!(!info.device_name.is_empty());
    }

    #[test]
    fn test_get_smbios_uuid() {
        let uuid = get_smbios_uuid();
        println!("SMBIOS UUID: {:?}", uuid);
        // UUID 可能为 None（权限或虚拟机限制）
    }

    #[test]
    fn test_get_primary_mac_address() {
        let mac = get_primary_mac_address();
        println!("Primary MAC: {:?}", mac);
        // MAC 地址可能为 None（无物理网卡）
    }

    #[test]
    fn test_get_boot_disk_serial() {
        let serial = get_boot_disk_serial();
        println!("Boot Disk Serial: {:?}", serial);
        // 序列号可能为 None（权限限制）
    }
}
