# CoupCat

**Un editor de video libre y de código abierto hecho en Rust** — edición al estilo
CapCut, muchos filtros y sin muros de pago. CoupCat sigue la filosofía *"nativo, no
envuelto"* de Artcraft: una aplicación de escritorio real, no un navegador dentro de
una caja. Una versión Web App (para Android y otros teléfonos) es un objetivo
planeado desde el mismo código.

> **Estado:** en desarrollo activo. El motor de edición compila y funciona; la
> interfaz de escritorio ya tiene importación, línea de tiempo, previsualización,
> filtros, texto, transiciones, audio, recorte de fondo y subtítulos. Mira la hoja
> de ruta más abajo.

## Cómo contribuir

Las contribuciones se coordinan con **issues** y **pull requests**: buscas (o creas)
un issue, **comentas para tomarlo**, trabajas en tu rama y abres un PR. El proceso
completo, pensado para que lo siga tanto una persona como un modelo de IA, está en
**[CONTRIBUTING.md](CONTRIBUTING.md)**.

## De dónde viene el código

CoupCat no parte de cero. Es un **trabajo derivado**: reutiliza el **motor de video
en Rust** de
[`artcraft-android/capcraft-android`](https://github.com/artcraft-android/capcraft-android)
(a su vez construido sobre el motor *Concat* de jub0t y colaboradores) — un motor de
video completo e independiente de plataforma, dividido en crates `concat-*`.
Conservamos ese motor y **reemplazamos la interfaz Android/Kotlin por una interfaz de
escritorio/web nueva en [Dioxus](https://dioxuslabs.com/)**.

CoupCat es un proyecto independiente, con su propio historial de Git. No está afiliado
ni respaldado por ArtCraft, el proyecto Concat, ByteDance ni CapCut.

**Licencia:** AGPL-3.0-or-later, heredada del motor original. CoupCat se mantiene
libre y de código abierto, conserva el copyright y la licencia de los autores
originales (como exige la AGPL) y publica sus propios aportes bajo la misma licencia.
Consulta `LICENSE`, `NOTICE` y `THIRD_PARTY_NOTICES.md`.

## El motor (`src/crates/`)

| Crate | Rol |
|-------|-----|
| `concat-core` | Tiempo, fotogramas y el modelo de línea de tiempo que comparte todo |
| `concat-project` | Datos del proyecto y de la línea de tiempo multipista |
| `concat-media` | Decodificar/codificar con **FFmpeg** (el único crate que sabe que FFmpeg existe) |
| `concat-render` | Convertir una línea de tiempo + marca de tiempo en un fotograma (**wgpu**) |
| `concat-effects` | Filtros y efectos, dibujados por shaders |
| `concat-text` | Títulos y texto |
| `concat-export` | Codificar la salida final y mezclar el audio |
| `concat-vision` | Modelos de recorte / eliminación de fondo *(IA en el dispositivo)* |
| `coupcat-desktop` | La aplicación de escritorio en Dioxus (interfaz, audio, subtítulos) |

Los crates `concat-host` y `concat-speech` del repositorio original se conservan como
referencia pero están **excluidos del build**: CoupCat hace esas tareas por su cuenta
(recorte de fondo con `concat-vision` directamente; subtítulos con `whisper-rs`).

### La interfaz (`src/crates/coupcat-desktop/src/`)

La ventana se reparte por regiones, cada una en su archivo, para que varias personas puedan
trabajar a la vez sin pisarse:

| Archivo | Qué contiene |
|---------|--------------|
| `main.rs` | Solo arranca la ventana (icono, tamaño, ONNX Runtime) |
| `app.rs` | El componente raíz: crea el estado, lanza las tareas en segundo plano y compone las regiones |
| `ui/ctx.rs` | `Ctx`: el estado compartido que leen y escriben las regiones |
| `ui/topbar.rs` | Barra superior: menú del proyecto, indicador de guardado, nombre del proyecto, atajos y exportar |
| `ui/banners.rs` | Avisos bajo la barra y diálogos: recuperación, mensajes de archivo, cambios sin guardar, atajos, errores, progreso |
| `ui/resources.rs` | Panel de recursos (izquierda): pestañas Importar, Audio, Texto, Efectos, Transiciones y Subtítulos, con categorías, buscador y tarjetas |
| `ui/stage.rs` | Reproductor: vista previa, tiempo, reproducir y barra de búsqueda |
| `ui/inspector.rs` | Panel derecho: texto, fondo (IA), subtítulos, audio, transiciones y efectos del clip |
| `ui/toolbar.rs` | Barra de herramientas de la línea de tiempo: deshacer, rehacer, dividir, eliminar y texto |
| `ui/timeline.rs` | Línea de tiempo: regla, cabeceras de pista, pistas, clips y barra de estado |
| `ui/files.rs` | Acciones de archivo: nuevo, abrir, guardar, recientes y recuperación |
| `ui/style.css` | Todo el CSS |

Las demás piezas (`editor.rs`, `monitor.rs`, `effects.rs`, `text.rs`, `captions.rs`,
`cutout.rs`, `export.rs`, `project_file.rs`…) son la lógica que la interfaz usa. Una región es
una función normal que recibe `&Ctx` y devuelve el `rsx!` de esa zona.

## Compilar en Windows

Compila con la **cadena de herramientas MSVC**, desde una *Developer Shell* de Visual
Studio (necesita `cl.exe`/`link.exe` y las cabeceras de MSVC para FFmpeg, ONNX Runtime
y whisper.cpp). La versión fijada (`rust-toolchain.toml`) resuelve al host MSVC;
`src/.cargo/config.toml` configura `FFMPEG_DIR` y `libclang`.

```powershell
$vc = "C:\Program Files\Microsoft Visual Studio\2022\Community"
Import-Module (Join-Path $vc "Common7\Tools\Microsoft.VisualStudio.DevShell.dll")
Enter-VsDevShell -VsInstallPath $vc -SkipAutomaticLocation -DevCmdArguments "-arch=x64 -host_arch=x64"
cd src
cargo run -p coupcat-desktop
```

Las dependencias locales bajo `third_party/` (ignoradas por Git, se descargan en cada
máquina): una **build de desarrollo compartida de FFmpeg** (BtbN), **ONNX Runtime**
(`onnxruntime-win-x64`, para el recorte de fondo — cargado dinámicamente) y un
**modelo de Whisper** (`ggml-base.bin`, para los subtítulos). `build.rs` copia las DLL
y el modelo junto al ejecutable automáticamente, y `scripts\package.ps1` arma un
`dist\CoupCat\` autocontenido para distribuir. `libclang` viene de Doxygen.

## Hoja de ruta

- **M0 — Andamiaje ✅** Motor derivado, cadena de herramientas de Windows, FFmpeg
  conectado, el motor base compila.
- **M1 — Shell de escritorio ✅** Ventana en Dioxus: importar, línea de tiempo y
  previsualización con reproducción.
- **M2 — Edición básica ✅** Recortar / dividir / mover (arrastrando con el ratón),
  multipista, deshacer/rehacer, previsualización compuesta, exportar MP4.
- **M3 — Filtros y efectos ✅** 162 efectos de shader con parámetros ajustables,
  texto/títulos y transiciones centradas al estilo CapCut.
- **M4 — IA en el dispositivo ✅** Eliminación de fondo (recorte de personas) y
  subtítulos automáticos (whisper.cpp), ambos locales. Cadena de herramientas MSVC.
- **M5 — Web App.** Objetivo WASM (WebCodecs / WebGPU) para navegadores de Android.
- **M6 — Pulido y empaquetado.** En progreso: dependencias de ejecución agrupadas
  automáticamente, script de empaquetado, tamaño de ventana. Pendiente: instalador y
  marca/ícono definitivos.

## Agradecimientos

- Motor *Concat* — Jareer Abdullah y los colaboradores de Concat.
- CapCraft para Android — el equipo de ArtCraft y los colaboradores de CapCraft.
- CoupCat — los colaboradores de CoupCat.
