# Synthia

A lightweight, local graphic design editor built with **Tauri, Rust, Yew, and SVG**.

Synthia is designed for creating posters, social media graphics, promotional designs, thumbnails, and other everyday visual content without requiring a cloud service or AI.


## Features

- Custom canvas sizes
- Canvas presets
- Canvas background colors
- Fit canvas to viewport
- Zoom controls
- Rectangle tool
- Ellipse tool
- Line tool
- Pencil/freehand tool
- Text tool
- Image import
- Layers
- Object selection
- Multi-selection
- Move, resize, and rotate objects
- Snapping and smart guides
- Alignment and distribution
- Text properties
- Image properties
- Undo / redo
- New / Open / Save / Save As
- PNG export
- SVG export

## Tech Stack

- **Rust** — application and editor logic
- **Tauri** — lightweight desktop application framework
- **Yew** — Rust/WebAssembly frontend
- **SVG** — vector graphics rendering

Synthia is intentionally designed to keep dependencies and resource usage low.

## Development

### Prerequisites

You will need the Rust/Tauri development environment for your operating system.

You will also need:

- Rust
- Cargo
- Trunk
- Tauri CLI

### Run

Start the Yew development server:

```bash
trunk serve
```

Or run the desktop application with Tauri:

```bash
cargo tauri dev
```

### Build

```bash
cargo tauri build
```

## Project Structure

```text
synthia/
├── src/
├── src-tauri/
├── Cargo.toml
├── Trunk.toml
└── README.md
```

## Philosophy

Synthia focuses on a simple local-first workflow:

- Fast
- Lightweight
- Desktop-first
- Local files
- Vector-based editing
- Simple interface

## Status

Synthia is currently under active development.

The core editor is functional, including canvas editing, shapes, text, images, layers, transformations, snapping, alignment, history, file operations, and export.

More advanced design and editing features are planned.

## Contributing

Issues, suggestions, and contributions are welcome.



## License

License information will be added as the project develops.
