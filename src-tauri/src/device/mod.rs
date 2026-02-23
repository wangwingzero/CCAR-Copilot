//! 设备指纹服务模块
//!
//! 生成唯一的设备标识符，用于设备授权管理。
//! 采用多因素组合：SMBIOS UUID + MAC 地址 + 磁盘序列号
//!
//! # 注意
//! 此模块为独立模块，暂不集成到 lib.rs（Phase 2 集成）

mod fingerprint;

pub use fingerprint::{generate_machine_id, get_device_info, DeviceInfo};
