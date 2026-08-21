//! Document → SVG serialization, and SVG → PNG rasterization, for the
//! Export feature.
//!
//! `document_to_svg` deliberately mirrors `canvas_area.rs`'s `render_object`
//! and friends — same transform math, same per-kind local-space content —
//! but emits plain markup text instead of Yew `Html`, since a real,
//! standalone `.svg` file (and a canvas to rasterize for PNG) needs actual
//! text, not virtual DOM nodes. Nothing here is the document's source of
//! truth; it's a one-way projection built fresh from `DesignDocument` on
//! every export, exactly like the canvas renderer is.

use serde::{Deserialize, Serialize};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Blob, BlobPropertyBag, HtmlCanvasElement, HtmlImageElement, Url};

use crate::model::{
    DesignDocument, DesignObject, Geometry, ImageFit, ImageProperties, LineProperties, ObjectKind, PathProperties, ShapeProperties,
    TextAlign, TextProperties, TextSizeMode,
};
use crate::tauri_bridge::invoke;
use crate::{freehand, text_metrics};

/// Escapes the characters that are special in both XML text content and
/// quoted attribute values — used for every piece of user-authored text
/// (object names aren't emitted, but text content and font names/colors
/// that could in principle contain them are handled uniformly rather than
/// trusting each call site to remember).
fn escape_xml(input: &str) -> String {
    input.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn transform_attr(g: &Geometry) -> String {
    format!("translate({} {}) rotate({} {} {})", g.x, g.y, g.rotation, g.width / 2.0, g.height / 2.0)
}

fn shape_svg(kind_tag: &str, props: &ShapeProperties, g: &Geometry) -> String {
    let stroke = if props.stroke.enabled {
        format!("stroke:{};stroke-width:{};", props.stroke.color.to_css(), props.stroke.width)
    } else {
        "stroke:none;".to_string()
    };
    let style = format!("fill:{};{stroke}", props.fill.to_css());
    match kind_tag {
        "rect" => format!(r#"<rect x="0" y="0" width="{}" height="{}" style="{}" />"#, g.width, g.height, style),
        _ => format!(
            r#"<ellipse cx="{}" cy="{}" rx="{}" ry="{}" style="{}" />"#,
            g.width / 2.0,
            g.height / 2.0,
            g.width / 2.0,
            g.height / 2.0,
            style
        ),
    }
}

fn line_svg(props: &LineProperties, g: &Geometry) -> String {
    let style = format!("stroke:{};stroke-width:{};stroke-linecap:round;", props.stroke_color.to_css(), props.stroke_width);
    format!(
        r#"<line x1="{}" y1="{}" x2="{}" y2="{}" style="{}" />"#,
        props.start.x * g.width,
        props.start.y * g.height,
        props.end.x * g.width,
        props.end.y * g.height,
        style
    )
}

fn path_svg(props: &PathProperties, g: &Geometry) -> String {
    let local_points: Vec<(f64, f64)> = props.points.iter().map(|p| (p.x * g.width, p.y * g.height)).collect();
    let d = freehand::smooth_path_d(&local_points);
    let style =
        format!("fill:none;stroke:{};stroke-width:{};stroke-linecap:round;stroke-linejoin:round;", props.stroke_color.to_css(), props.stroke_width);
    format!(r#"<path d="{}" style="{}" />"#, d, style)
}

fn text_svg(props: &TextProperties, g: &Geometry) -> String {
    let lines: Vec<String> = match props.size_mode {
        TextSizeMode::Auto => props.content.split('\n').map(str::to_string).collect(),
        TextSizeMode::Fixed => text_metrics::wrap_lines(props, g.width),
    };
    let line_height_px = props.font_size * props.line_height;
    let (x, anchor) = match props.text_align {
        TextAlign::Left => (text_metrics::HORIZONTAL_PADDING, "start"),
        TextAlign::Center => (g.width / 2.0, "middle"),
        TextAlign::Right => (g.width - text_metrics::HORIZONTAL_PADDING, "end"),
    };
    let style = format!(
        "font-family:{};font-weight:{};font-style:{};letter-spacing:{}px;text-decoration:{};fill:{};",
        props.font_family.css_stack(),
        props.font_weight.css_value(),
        props.font_style.css_value(),
        props.letter_spacing,
        props.text_decoration.css_value(),
        props.fill.to_css(),
    );
    let tspans: String = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            format!(
                r#"<tspan x="{}" y="{}">{}</tspan>"#,
                x,
                text_metrics::VERTICAL_PADDING + (i as f64 + 0.8) * line_height_px,
                escape_xml(line)
            )
        })
        .collect();
    format!(r#"<text x="{x}" y="0" text-anchor="{anchor}" font-size="{}" style="{style}">{tspans}</text>"#, props.font_size)
}

/// An image's own reference is a browser object URL (see
/// `asset_manager.rs`'s module docs) — valid for PNG export, which
/// rasterizes within the same session, but a saved `.svg` file's `<image
/// href>` won't resolve once the session ends. Same disclosed tradeoff
/// `document_io.rs` already accepts for save/open; not re-solved here.
fn image_svg(props: &ImageProperties, g: &Geometry, document: &DesignDocument) -> String {
    let Some(asset) = document.assets.get(props.asset_id) else {
        return String::new();
    };
    let preserve_aspect_ratio = match props.fit {
        ImageFit::Fill => "none",
        ImageFit::Fit => "xMidYMid meet",
        ImageFit::Crop => "xMidYMid slice",
    };
    format!(
        r#"<image href="{}" x="0" y="0" width="{}" height="{}" preserveAspectRatio="{}" />"#,
        escape_xml(&asset.reference),
        g.width,
        g.height,
        preserve_aspect_ratio
    )
}

fn object_svg(object: &DesignObject, document: &DesignDocument) -> String {
    if object.kind.is_group() {
        return String::new();
    }
    let g = &object.geometry;
    let inner = match &object.kind {
        ObjectKind::Rectangle(props) => shape_svg("rect", props, g),
        ObjectKind::Ellipse(props) => shape_svg("ellipse", props, g),
        ObjectKind::Line(props) => line_svg(props, g),
        ObjectKind::Path(props) => path_svg(props, g),
        ObjectKind::Text(props) => text_svg(props, g),
        ObjectKind::Image(props) => image_svg(props, g, document),
        ObjectKind::Group => String::new(),
    };
    format!(r#"<g transform="{}" opacity="{}">{}</g>"#, transform_attr(g), g.opacity, inner)
}

/// Serializes the whole document into a standalone SVG document string —
/// same z-order (document list order), same per-object transform/opacity,
/// as the live canvas. The document itself stays the only source of
/// truth; this is rebuilt fresh on every export, never cached.
pub fn document_to_svg(document: &DesignDocument) -> String {
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{0}" height="{1}" viewBox="0 0 {0} {1}"><rect x="0" y="0" width="{0}" height="{1}" fill="{2}" />"#,
        document.canvas_width,
        document.canvas_height,
        escape_xml(&document.background)
    );
    for object in &document.objects {
        svg.push_str(&object_svg(object, document));
    }
    svg.push_str("</svg>");
    svg
}

/// Waits for `image`'s `load` (or `error`) event via a hand-built
/// `Promise` — the standard wasm-bindgen pattern for bridging a DOM
/// callback into `async`/`await`, used here instead of a crate since it's
/// a dozen lines and this is the only place Apollo needs it.
async fn wait_for_load(image: &HtmlImageElement) -> Result<(), String> {
    let image_ok = image.clone();
    let image_err = image.clone();
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let onload = Closure::once(move || {
            let _ = resolve.call0(&JsValue::NULL);
        });
        image_ok.set_onload(Some(onload.as_ref().unchecked_ref()));
        onload.forget();

        let onerror = Closure::once(move || {
            let _ = reject.call0(&JsValue::NULL);
        });
        image_err.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();
    });
    wasm_bindgen_futures::JsFuture::from(promise).await.map(|_| ()).map_err(|_| "Could not rasterize the design for PNG export.".to_string())
}

/// Rasterizes `document` to PNG bytes: serialize to SVG, load it as an
/// `<img>` (via a `Blob` URL, never a data URL — sidesteps having to
/// percent/base64-encode arbitrary SVG text), draw it into an offscreen
/// canvas sized to the artboard, and read the canvas back out as PNG.
/// Every object stays a real vector shape right up until this one
/// rasterization step, same as the "never rasterize until export" rule
/// for the `<image>` element the canvas renderer already follows.
pub async fn document_to_png(document: &DesignDocument) -> Result<Vec<u8>, String> {
    let svg_text = document_to_svg(document);

    let parts = js_sys::Array::new();
    parts.push(&JsValue::from_str(&svg_text));
    let blob_options = BlobPropertyBag::new();
    blob_options.set_type("image/svg+xml");
    let blob = Blob::new_with_str_sequence_and_options(&parts, &blob_options).map_err(|_| "Could not prepare the design for export.".to_string())?;
    let url = Url::create_object_url_with_blob(&blob).map_err(|_| "Could not prepare the design for export.".to_string())?;

    let image = HtmlImageElement::new().map_err(|_| "Could not prepare the design for export.".to_string())?;
    let load = wait_for_load(&image);
    image.set_src(&url);
    let load_result = load.await;
    let _ = Url::revoke_object_url(&url);
    load_result?;

    let window = web_sys::window().ok_or("Not running in a browser.")?;
    let document_el = window.document().ok_or("Not running in a browser.")?;
    let canvas = document_el
        .create_element("canvas")
        .map_err(|_| "Could not prepare the design for export.".to_string())?
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| "Could not prepare the design for export.".to_string())?;
    canvas.set_width(document.canvas_width.round().max(1.0) as u32);
    canvas.set_height(document.canvas_height.round().max(1.0) as u32);

    let context = canvas
        .get_context("2d")
        .map_err(|_| "Could not prepare the design for export.".to_string())?
        .ok_or("Could not prepare the design for export.")?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .map_err(|_| "Could not prepare the design for export.".to_string())?;
    context
        .draw_image_with_html_image_element(&image, 0.0, 0.0)
        .map_err(|_| "Could not draw the design for export.".to_string())?;

    let data_url = canvas.to_data_url_with_type("image/png").map_err(|_| "Could not encode the exported PNG.".to_string())?;
    let base64 = data_url.split(',').nth(1).ok_or("Could not encode the exported PNG.")?;
    let binary_string = window.atob(base64).map_err(|_| "Could not encode the exported PNG.".to_string())?;
    Ok(binary_string.chars().map(|c| c as u8).collect())
}

#[derive(Serialize)]
struct ExportArgs {
    contents: Vec<u8>,
    suggested_name: String,
    extension: String,
    filter_name: String,
}

#[derive(Deserialize)]
struct ExportResponse {
    #[allow(dead_code)]
    path: String,
}

pub enum ExportOutcome {
    Exported,
    /// The user closed the native save dialog without picking a location.
    Cancelled,
}

async fn export_bytes(contents: Vec<u8>, suggested_name: String, extension: &str, filter_name: &str) -> Result<ExportOutcome, String> {
    let args = ExportArgs { contents, suggested_name, extension: extension.to_string(), filter_name: filter_name.to_string() };
    match invoke::<_, Option<ExportResponse>>("export_file", &args).await {
        Ok(Some(_)) => Ok(ExportOutcome::Exported),
        Ok(None) => Ok(ExportOutcome::Cancelled),
        Err(error) => Err(error.to_string()),
    }
}

/// Exports the document as a standalone `.svg` file via the native save
/// dialog.
pub async fn export_svg(document: &DesignDocument, suggested_name: String) -> Result<ExportOutcome, String> {
    export_bytes(document_to_svg(document).into_bytes(), suggested_name, "svg", "SVG Image").await
}

/// Exports the document as a rasterized `.png` file via the native save
/// dialog.
pub async fn export_png(document: &DesignDocument, suggested_name: String) -> Result<ExportOutcome, String> {
    let bytes = document_to_png(document).await?;
    export_bytes(bytes, suggested_name, "png", "PNG Image").await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Color, DesignDocument, Geometry, ObjectKind, ShapeProperties};

    #[test]
    fn escapes_special_characters() {
        assert_eq!(escape_xml(r#"a & b < c > d " e"#), "a &amp; b &lt; c &gt; d &quot; e");
    }

    #[test]
    fn serializes_a_rectangle_with_its_fill() {
        let mut document = DesignDocument::default();
        let mut props = ShapeProperties::default();
        props.fill = Color::rgb(0x11, 0x22, 0x33);
        document.insert(ObjectKind::Rectangle(props), Geometry::new(10.0, 20.0, 100.0, 50.0));

        let svg = document_to_svg(&document);
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("<rect"));
        assert!(svg.contains("translate(10 20)"));
        assert!(svg.contains("rgba(17, 34, 51, 1)"));
    }

    #[test]
    fn skips_groups_but_keeps_their_children() {
        let mut document = DesignDocument::default();
        let a = document.insert(ObjectKind::Rectangle(ShapeProperties::default()), Geometry::new(0.0, 0.0, 10.0, 10.0));
        let b = document.insert(ObjectKind::Ellipse(ShapeProperties::default()), Geometry::new(20.0, 0.0, 10.0, 10.0));
        document.group(&[a, b]).expect("grouping 2 objects should succeed");

        let svg = document_to_svg(&document);
        // Both children still render; the group object itself (no shape of
        // its own) contributes no markup. `<rect` count is 2: the artboard
        // background rect plus the one rectangle object.
        assert_eq!(svg.matches("<rect").count(), 2);
        assert_eq!(svg.matches("<ellipse").count(), 1);
    }
}
