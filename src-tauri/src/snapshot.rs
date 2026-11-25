//! 网页当前样子的截图（换配色的水面扭曲用，`decisions/0033`）：macOS 上让 WKWebView 拍一张，原始像素交给前端的 WebGL，
//! 在 GPU 上逐像素扭（WebKit 的 SVG 滤镜在 CPU 上算，整页每帧约 100 毫秒）。
//! 返回的字节：前 12 个是三个 u32（小端）：宽、高、像素的排法（0 = RGBA，1 = BGRA，2 = ARGB），后面是一行接一行、没有空隙的像素。

#[tauri::command]
pub async fn app_snapshot(webview: tauri::Webview) -> Result<tauri::ipc::Response, String> {
    #[cfg(target_os = "macos")]
    {
        mac::snapshot(&webview).await.map(tauri::ipc::Response::new)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = webview;
        Err("只在 macOS 上能拍".into())
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::Cell;
    use std::sync::mpsc;
    use std::time::Duration;

    use block2::RcBlock;
    use objc2_app_kit::NSImage;
    use objc2_core_graphics::{CGDataProvider, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo};
    use objc2_foundation::NSError;
    use objc2_web_kit::WKWebView;

    pub async fn snapshot(webview: &tauri::Webview) -> Result<Vec<u8>, String> {
        let (tx, rx) = mpsc::sync_channel::<Result<Vec<u8>, String>>(1);
        webview
            .with_webview(move |pv| {
                // 主线程上：拿到 WKWebView，拍完的回调也在主线程
                let wk = unsafe { &*(pv.inner() as *const WKWebView) };
                let tx = Cell::new(Some(tx));
                let done = RcBlock::new(move |img: *mut NSImage, _err: *mut NSError| {
                    let r = unsafe { img.as_ref() }.ok_or_else(|| "没有拍到".to_string()).and_then(pixels);
                    if let Some(tx) = tx.take() {
                        let _ = tx.send(r);
                    }
                });
                unsafe { wk.takeSnapshotWithConfiguration_completionHandler(None, &done) };
            })
            .map_err(|e| e.to_string())?;
        tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(2)).map_err(|_| "截图超时".to_string())?)
            .await
            .map_err(|e| e.to_string())?
    }

    /// NSImage → 一行接一行的像素，前面是宽、高、排法
    fn pixels(img: &NSImage) -> Result<Vec<u8>, String> {
        let cg = unsafe { img.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None) }.ok_or("拿不到位图")?;
        let (w, h, row) = (CGImage::width(Some(&cg)), CGImage::height(Some(&cg)), CGImage::bytes_per_row(Some(&cg)));
        if CGImage::bits_per_pixel(Some(&cg)) != 32 || w == 0 || h == 0 {
            return Err("位图不是每像素 32 位".into());
        }
        let alpha = CGImage::alpha_info(Some(&cg));
        let little = CGImage::byte_order_info(Some(&cg)) == CGImageByteOrderInfo::Order32Little;
        let first = matches!(alpha, CGImageAlphaInfo::PremultipliedFirst | CGImageAlphaInfo::First | CGImageAlphaInfo::NoneSkipFirst);
        // 小端 + alpha 在前 = 内存里 B G R A；默认（大端）+ alpha 在后 = R G B A；大端 + alpha 在前 = A R G B
        let order: u32 = match (little, first) {
            (true, true) => 1,
            (false, false) => 0,
            (false, true) => 2,
            (true, false) => return Err("不认识的像素排法".into()),
        };
        let data = CGDataProvider::data(CGImage::data_provider(Some(&cg)).as_deref()).ok_or("读不到像素")?;
        let bytes = data.to_vec();
        let mut out = Vec::with_capacity(12 + w * h * 4);
        out.extend_from_slice(&(w as u32).to_le_bytes());
        out.extend_from_slice(&(h as u32).to_le_bytes());
        out.extend_from_slice(&order.to_le_bytes());
        for y in 0..h {
            out.extend_from_slice(bytes.get(y * row..y * row + w * 4).ok_or("像素不够")?);
        }
        Ok(out)
    }
}
