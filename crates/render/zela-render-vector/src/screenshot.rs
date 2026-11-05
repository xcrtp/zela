//! **CH-d**：debug-only 截图旁路（仅 Vector 后端）。
//!
//! # 为什么是"旁路"
//!
//! 主路径刻意零 CPU 往返：矢量帧渲进中间纹理后**直接注册为 egui 原生纹理**。
//! 截图是**按需回读**，只走 debug 通道（`ZELA_DEBUG_RPC`），不进主循环。
//!
//! # 边界
//!
//! - **只覆盖 Vector 后端**（Wgpu 那条另开出口，记入 GAPS）；
//! - **PNG 不进逐位哈希基线**：像素受字体 / AA / GPU 影响，只作人眼 / 演示 /
//!   视觉回归证据（结构化断言仍然读 Frame）。
//! - 非 wasm：浏览器没有阻塞读回（走 `Player::pixels_async`），本旁路不参与。

/// 去掉 `copy_texture_to_buffer` 的行对齐填充（GPU 要求 `bytes_per_row` 是
/// 256 的倍数，PNG 要紧凑行）。
///
/// **CJ-05（B6）**：公开面带长度校验 —— 短缓冲返回显式错误，不 panic。
pub fn unpad_rows(
    padded: &[u8],
    width: u32,
    height: u32,
    padded_row: u32,
) -> Result<Vec<u8>, String> {
    let unpadded = (width as usize)
        .checked_mul(4)
        .ok_or_else(|| format!("行宽溢出：width = {width}"))?;
    if (padded_row as usize) < unpadded {
        return Err(format!(
            "行对齐填充 {padded_row} 小于紧凑行宽 {unpadded}（width = {width}）"
        ));
    }
    let needed = (height as usize)
        .checked_mul(padded_row as usize)
        .ok_or_else(|| format!("缓冲长度溢出：height = {height}, padded_row = {padded_row}"))?;
    if padded.len() < needed {
        return Err(format!(
            "缓冲太短：需要 {needed} 字节（height × padded_row），实际 {}",
            padded.len()
        ));
    }
    let mut out = Vec::with_capacity(unpadded * height as usize);
    for row in 0..height as usize {
        let start = row * padded_row as usize;
        out.extend_from_slice(&padded[start..start + unpadded]);
    }
    Ok(out)
}

/// **debug-only 回读**：RGBA8 纹理 → 紧凑像素（不带行填充）。
///
/// 这是本 crate 里第二处"同步三连"（`map_async` → `poll(wait)` →
/// `get_mapped_range`）；第一处是测试用的 `Gpu::read_buffer`。
#[cfg(not(target_arch = "wasm32"))]
pub fn read_texture_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("zela-debug-screenshot"),
        size: u64::from(padded_row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("zela-debug-screenshot"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| error.to_string())?;
    let mapped = slice
        .get_mapped_range()
        .map_err(|error| error.to_string())?;
    let rgba = unpad_rows(&mapped, width, height, padded_row)?;
    drop(mapped);
    buffer.unmap();
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpadding_drops_the_gpu_row_filler() {
        // 宽 2：8 字节/行，GPU 行对齐 256 ⇒ padded_row = 256。
        let mut padded = vec![0u8; 256 * 2];
        padded[0..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        padded[256..264].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);
        let rgba = unpad_rows(&padded, 2, 2, 256).expect("缓冲足够");
        assert_eq!(
            rgba,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
    }

    /// **CJ-05（B6）**：短缓冲 / 非法行宽返回显式错误，不 panic。
    #[test]
    fn unpadding_rejects_short_buffers_and_narrow_rows() {
        let short = vec![0u8; 8];
        assert!(unpad_rows(&short, 2, 2, 256).is_err(), "短缓冲必须报错");
        assert!(
            unpad_rows(&[0u8; 256], 2, 1, 4).is_err(),
            "行宽小于紧凑行宽必须报错"
        );
    }
}
