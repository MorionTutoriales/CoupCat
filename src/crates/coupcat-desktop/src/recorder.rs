// SPDX-License-Identifier: AGPL-3.0-or-later
//
//! Screen recording (#51): the `ffmpeg.exe` shipped beside the app captures a
//! monitor (`ddagrab`, Desktop Duplication) and, if asked, a microphone
//! (DirectShow) and the computer's own sound ([`system_audio`]). The take is
//! written as Matroska, which stays readable if the recording is cut short,
//! with each sound in its own track, and ffmpeg's log notes the wall-clock
//! moment each source began. Once it ends it becomes an MP4: the picture is
//! copied, and the sounds are lined up with it by those moments and mixed.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::system_audio;

/// A monitor that can be captured: which graphics adapter drives it, its
/// number on that adapter, and its size in physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub adapter: u32,
    pub output: u32,
    pub width: u32,
    pub height: u32,
}

/// What this machine offers to record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Devices {
    pub screens: Vec<Screen>,
    pub mics: Vec<String>,
    /// The output device whose sound can be recorded, if there is one.
    pub system: Option<String>,
}

/// One take's settings.
#[derive(Clone, Debug)]
pub struct Settings {
    pub screen: Screen,
    pub mic: Option<String>,
    /// Whether to record the computer's own sound.
    pub system: bool,
    pub fps: u32,
    pub cursor: bool,
}

/// The hardware H.264 encoders, in order of preference; x264 is the fallback.
const HARDWARE: &[&str] = &["h264_nvenc", "h264_qsv", "h264_amf"];
const SOFTWARE: &str = "libx264";

/// How long a new capture is watched for an immediate failure.
const START_WATCH: Duration = Duration::from_millis(1800);
/// How late DirectShow's samples are stamped, in seconds: its buffer has to
/// fill first.
const MIC_LATENCY: f64 = 0.05;
/// The sample rate of the finished sound.
const RATE: u32 = 48_000;
/// How long ffmpeg gets to close the file after being told to stop.
const STOP_WAIT: Duration = Duration::from_secs(30);

/// The ffmpeg program: beside the app (where the build and the package put
/// it), else whatever the system finds.
fn ffmpeg() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        // Test binaries live one folder down, in `deps`.
        for dir in exe.ancestors().skip(1).take(2) {
            let candidate = dir.join("ffmpeg.exe");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from("ffmpeg")
}

fn command() -> Command {
    let mut c = Command::new(ffmpeg());
    c.args(["-hide_banner", "-nostats"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// Where takes go unless the person picks another folder.
pub fn default_folder() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Videos")
        .join("CoupCat")
        .join("Grabaciones")
}

/// List the monitors and microphones. Takes a couple of seconds.
pub fn devices() -> Result<Devices, String> {
    let listing = command()
        .args(["-list_devices", "true", "-f", "dshow", "-i", "dummy"])
        .output()
        .map_err(|e| format!("No se pudo ejecutar ffmpeg: {e}"))?;
    let mics = parse_mics(&String::from_utf8_lossy(&listing.stderr));

    let mut screens = Vec::new();
    for adapter in 0..4 {
        for output in 0..8 {
            match probe_screen(adapter, output) {
                Some(screen) => screens.push(screen),
                None => break,
            }
        }
    }
    if screens.is_empty() {
        return Err("No se encontró ninguna pantalla que se pueda capturar.".to_string());
    }
    Ok(Devices {
        screens,
        mics,
        system: system_audio::device_name(),
    })
}

fn probe_screen(adapter: u32, output: u32) -> Option<Screen> {
    let run = command()
        .args(["-init_hw_device", &format!("d3d11va=d:{adapter}")])
        .args(["-filter_hw_device", "d"])
        .args([
            "-filter_complex",
            &format!("ddagrab=output_idx={output},hwdownload,format=bgra"),
        ])
        .args(["-frames:v", "1", "-f", "null", "-"])
        .output()
        .ok()?;
    if !run.status.success() {
        return None;
    }
    let (width, height) = parse_size(&String::from_utf8_lossy(&run.stderr))?;
    Some(Screen {
        adapter,
        output,
        width,
        height,
    })
}

/// The microphone names in ffmpeg's DirectShow listing.
fn parse_mics(listing: &str) -> Vec<String> {
    listing
        .lines()
        .filter_map(|line| line.trim_end().strip_suffix("\" (audio)"))
        .filter_map(|line| line.split_once('"').map(|(_, name)| name.to_string()))
        .collect()
}

/// The picture size in the line ffmpeg prints for a video stream.
fn parse_size(log: &str) -> Option<(u32, u32)> {
    log.lines()
        .filter(|l| l.contains("Video:"))
        .flat_map(|l| l.split([' ', ',']))
        .find_map(|word| {
            let (w, h) = word.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
}

/// The best H.264 encoder that actually works here, found once.
fn encoder() -> &'static str {
    static FOUND: OnceLock<&'static str> = OnceLock::new();
    FOUND.get_or_init(|| {
        HARDWARE
            .iter()
            .copied()
            .find(|name| {
                command()
                    .args(["-f", "lavfi", "-i", "color=size=1920x1080:rate=30"])
                    .args(["-frames:v", "3", "-pix_fmt", "nv12", "-c:v", name])
                    .args(["-f", "null", "-"])
                    .output()
                    .is_ok_and(|o| o.status.success())
            })
            .unwrap_or(SOFTWARE)
    })
}

/// Constant-quality options for each encoder.
fn quality(encoder: &str) -> &'static [&'static str] {
    match encoder {
        "h264_nvenc" => &["-preset", "p4", "-cq", "23", "-b:v", "0"],
        "h264_qsv" => &["-global_quality", "23"],
        "h264_amf" => &["-rc", "cqp", "-qp_i", "22", "-qp_p", "22"],
        _ => &["-preset", "veryfast", "-crf", "21"],
    }
}

/// The ffmpeg arguments that record `settings` into `out`. `system` holds the
/// input arguments of the computer's sound, when it is recorded.
fn capture_args(
    settings: &Settings,
    system: Option<&[String]>,
    encoder: &str,
    out: &Path,
) -> Vec<String> {
    let s = &settings.screen;
    let mut a: Vec<String> = vec![
        // The log carries the starting moments `source_starts` reads.
        "-loglevel".into(),
        "info".into(),
        "-y".into(),
        "-init_hw_device".into(),
        format!("d3d11va=d:{}", s.adapter),
        "-filter_hw_device".into(),
        "d".into(),
    ];
    if let Some(mic) = &settings.mic {
        a.extend(
            [
                "-use_wallclock_as_timestamps",
                "1",
                "-f",
                "dshow",
                "-rtbufsize",
                "100M",
                "-audio_buffer_size",
                "50",
                "-i",
            ]
            .map(String::from),
        );
        a.push(format!("audio={mic}"));
    }
    if let Some(input) = system {
        a.extend_from_slice(input);
    }
    // H.264 wants even sides, so an odd pixel is cropped away. A copy of the
    // first frame goes to the log stamped with the wall clock.
    let mut filter = format!(
        "ddagrab=output_idx={}:framerate={}:draw_mouse={},hwdownload,format=bgra,\
         crop=trunc(iw/2)*2:trunc(ih/2)*2,format=nv12,split=2[v][first];\
         [first]trim=end_frame=1,settb=1/1000000,setpts=RTCTIME,showinfo,nullsink",
        s.output,
        settings.fps,
        u8::from(settings.cursor),
    );
    let mut tracks = Vec::new();
    if settings.mic.is_some() {
        // Stretched as needed to follow the clock.
        filter.push_str(&format!(";[0:a]aresample={RATE}:async=1000[m]"));
        tracks.push("[m]");
    }
    if system.is_some() {
        let index = u8::from(settings.mic.is_some());
        filter.push_str(&format!(";[{index}:a]aresample={RATE}[s]"));
        tracks.push("[s]");
    }
    a.push("-filter_complex".into());
    a.push(filter);
    a.extend(["-map", "[v]"].map(String::from));
    for track in &tracks {
        a.extend(["-map", track].map(String::from));
    }
    if !tracks.is_empty() {
        a.extend(["-c:a", "aac", "-b:a", "192k"].map(String::from));
    }
    a.push("-c:v".into());
    a.push(encoder.into());
    a.extend(quality(encoder).iter().map(|s| s.to_string()));
    // Write to disk as it goes (a key frame and a closed cluster each second
    // or two), so a take cut short still plays up to its last moments.
    a.push("-g".into());
    a.push((settings.fps * 2).to_string());
    a.extend(["-cluster_time_limit", "1000", "-flush_packets", "1"].map(String::from));
    a.push(out.to_string_lossy().into_owned());
    a
}

/// The sounds in a take, in the order of its tracks.
#[derive(Clone, Copy)]
struct Sources {
    mic: bool,
    /// When the computer's sound began, in wall-clock microseconds.
    system: Option<u128>,
}

/// For each sound track, how many seconds before the picture it began,
/// according to ffmpeg's `log`. A moment the log lacks counts as no offset.
fn leads(sources: Sources, log: &str) -> Vec<f64> {
    // The copy of the first frame that `showinfo` describes.
    let picture = number_after(log, "pts_time:");
    let lead = |began: Option<f64>| match (picture, began) {
        (Some(p), Some(b)) => p - b,
        _ => 0.0,
    };
    let mut all = Vec::new();
    if sources.mic {
        // The microphone is the first input; its start is the arrival of its
        // first samples, which waited for the device's buffer to fill.
        let began = number_after(log, ", start: ").map(|t| t - MIC_LATENCY);
        all.push(lead(began));
    }
    if let Some(epoch) = sources.system {
        all.push(lead(Some(epoch as f64 / 1e6)));
    }
    all
}

/// The number that follows the first `label` in `log`.
fn number_after(log: &str, label: &str) -> Option<f64> {
    let rest = log.split_once(label)?.1.trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// The arguments that turn a take into the finished file, given how long before
/// the picture each of its sounds began: each is cut or padded to start with the
/// picture, and they are mixed at their own levels.
fn finish_args(leads: &[f64]) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    if !leads.is_empty() {
        let mut filter = String::new();
        for (i, lead) in leads.iter().enumerate() {
            let first = (lead * f64::from(RATE)).round() as i64;
            filter.push_str(&format!(
                "[0:a:{i}]aresample={RATE}:async=1:first_pts={first},asetpts=PTS-({first})[a{i}];"
            ));
        }
        for i in 0..leads.len() {
            filter.push_str(&format!("[a{i}]"));
        }
        if leads.len() > 1 {
            filter.push_str(&format!(
                "amix=inputs={}:duration=longest:dropout_transition=0:normalize=0[a]",
                leads.len()
            ));
        } else {
            filter.push_str("anull[a]");
        }
        a.extend(["-filter_complex".to_string(), filter]);
    }
    a.extend(["-map", "0:v", "-c:v", "copy"].map(String::from));
    if !leads.is_empty() {
        a.extend(["-map", "[a]", "-c:a", "aac", "-b:a", "192k", "-shortest"].map(String::from));
    }
    a.extend(["-movflags", "+faststart"].map(String::from));
    a
}

/// A capture in progress. Dropping it stops ffmpeg and keeps the file.
pub struct Recording {
    child: Child,
    stdin: Option<ChildStdin>,
    /// The Matroska file being written, and ffmpeg's messages beside it.
    mkv: PathBuf,
    log: PathBuf,
    started: Instant,
    #[cfg(windows)]
    _job: Option<job::Job>,
    /// Feeds ffmpeg the computer's sound; dropped after ffmpeg has ended.
    _system: Option<system_audio::Capture>,
    sources: Sources,
}

/// Start recording into `folder`, in a file named after `name`.
pub fn start(settings: &Settings, folder: &Path, name: &str) -> Result<Recording, String> {
    fs::create_dir_all(folder)
        .map_err(|e| format!("No se pudo crear la carpeta {}: {e}", folder.display()))?;
    let mkv = free_path(folder, name);
    let best = encoder();
    match launch(settings, best, &mkv) {
        // A hardware encoder can pass the check and still refuse real work.
        Err(_) if best != SOFTWARE => launch(settings, SOFTWARE, &mkv),
        other => other,
    }
}

/// A `.mkv` path in `folder` where neither it nor its `.mp4` exists yet.
fn free_path(folder: &Path, name: &str) -> PathBuf {
    let mut n = 1;
    loop {
        let stem = if n == 1 {
            name.to_string()
        } else {
            format!("{name} ({n})")
        };
        let mkv = folder.join(format!("{stem}.mkv"));
        if !mkv.exists() && !mkv.with_extension("mp4").exists() {
            return mkv;
        }
        n += 1;
    }
}

fn launch(settings: &Settings, encoder: &str, mkv: &Path) -> Result<Recording, String> {
    let log = mkv.with_extension("log");
    let log_file = File::create(&log).map_err(|e| format!("No se pudo escribir: {e}"))?;
    let system = if settings.system {
        Some(system_audio::Capture::open()?)
    } else {
        None
    };
    let input = system.as_ref().map(system_audio::Capture::input_args);
    let mut child = command()
        .args(capture_args(settings, input.as_deref(), encoder, mkv))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_file))
        .spawn()
        .map_err(|e| format!("No se pudo ejecutar ffmpeg: {e}"))?;
    #[cfg(windows)]
    let _job = job::Job::holding(&child);
    let stdin = child.stdin.take();
    let mut recording = Recording {
        child,
        stdin,
        mkv: mkv.to_path_buf(),
        log,
        started: Instant::now(),
        #[cfg(windows)]
        _job,
        sources: Sources {
            mic: settings.mic.is_some(),
            system: system.as_ref().map(system_audio::Capture::epoch),
        },
        _system: system,
    };
    while recording.started.elapsed() < START_WATCH {
        if let Some(why) = recording.failed() {
            let _ = fs::remove_file(&recording.mkv);
            let _ = fs::remove_file(&recording.log);
            return Err(why);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(recording)
}

impl Recording {
    /// Seconds since the capture began.
    pub fn seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Why ffmpeg stopped by itself, if it did.
    pub fn failed(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(None) => None,
            _ => {
                let log = fs::read_to_string(&self.log).unwrap_or_default();
                let mut tail: Vec<&str> = log.lines().rev().take(3).collect();
                tail.reverse();
                Some(format!("La captura se detuvo. {}", tail.join(" ")))
            }
        }
    }

    /// Ask ffmpeg to close the file and wait for it.
    fn finish(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.write_all(b"q\n");
            let _ = stdin.flush();
        }
        let asked = Instant::now();
        while matches!(self.child.try_wait(), Ok(None)) {
            if asked.elapsed() > STOP_WAIT {
                let _ = self.child.kill();
                let _ = self.child.wait();
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// End the take and return the finished file: an MP4, or the Matroska
    /// original if it could not be converted.
    pub fn stop(mut self) -> Result<PathBuf, String> {
        self.finish();
        let size = fs::metadata(&self.mkv).map(|m| m.len()).unwrap_or(0);
        if size == 0 {
            let why = fs::read_to_string(&self.log).unwrap_or_default();
            return Err(format!("No se grabó nada. {}", why.trim()));
        }
        let mp4 = self.mkv.with_extension("mp4");
        let notes = fs::read(&self.log).unwrap_or_default();
        let leads = leads(self.sources, &String::from_utf8_lossy(&notes));
        let converted = command()
            .args(["-loglevel", "error", "-y", "-i"])
            .arg(&self.mkv)
            .args(finish_args(&leads))
            .arg(&mp4)
            .output()
            .is_ok_and(|o| o.status.success());
        let _ = fs::remove_file(&self.log);
        if converted {
            let _ = fs::remove_file(&self.mkv);
            Ok(mp4)
        } else {
            let _ = fs::remove_file(&mp4);
            Ok(self.mkv.clone())
        }
    }

    /// Throw the take away.
    pub fn cancel(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.stdin = None;
        let _ = fs::remove_file(&self.mkv);
        let _ = fs::remove_file(&self.log);
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        // After `stop` or `cancel` the process is gone and this does nothing.
        if matches!(self.child.try_wait(), Ok(None)) {
            self.finish();
        }
    }
}

/// A Windows job that ends ffmpeg if the app dies without stopping it, so no
/// invisible recording is left running.
#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    pub struct Job(HANDLE);

    // A job handle is a kernel object any thread may close.
    unsafe impl Send for Job {}

    impl Job {
        pub fn holding(child: &Child) -> Option<Job> {
            // SAFETY: plain Win32 calls on a handle we create and on the
            // child's own handle; `info` outlives the call that reads it.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return None;
                }
                let job = Job(handle);
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let set = SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_ref(&info).cast(),
                    size_of_val(&info) as u32,
                );
                let assigned = AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE);
                (set != 0 && assigned != 0).then_some(job)
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle is ours and closed once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_microphones_from_the_listing() {
        let log = "[in#0 @ 1] \"FHD Camera\" (video)\n[in#0 @ 1]   Alternative name \"@x\"\n\
                   [in#0 @ 1] \"Micrófono (USB (2))\" (audio)\r\n";
        assert_eq!(parse_mics(log), vec!["Micrófono (USB (2))".to_string()]);
    }

    #[test]
    fn reads_the_screen_size() {
        let log = "  Stream #0:0: Video: wrapped_avframe, bgra(pc, gbr), 1920x1200, 30 fps";
        assert_eq!(parse_size(log), Some((1920, 1200)));
        assert_eq!(parse_size("nothing here"), None);
    }

    #[test]
    fn arguments_include_audio_only_with_a_microphone() {
        let mut s = Settings {
            screen: Screen {
                adapter: 1,
                output: 0,
                width: 1920,
                height: 1080,
            },
            mic: None,
            system: false,
            fps: 60,
            cursor: false,
        };
        let silent = capture_args(&s, None, SOFTWARE, Path::new("a.mkv")).join(" ");
        assert!(silent.contains("d3d11va=d:1"));
        assert!(silent.contains("framerate=60:draw_mouse=0"));
        assert!(!silent.contains("dshow") && !silent.contains("-c:a"));
        s.mic = Some("Mic".to_string());
        let voiced = capture_args(&s, None, SOFTWARE, Path::new("a.mkv")).join(" ");
        assert!(voiced.contains("-i audio=Mic"));
        assert!(voiced.contains("-map [m] -c:a aac"));
        let system = system_audio::input_args(48000, 5000);
        let both = capture_args(&s, Some(&system), SOFTWARE, Path::new("a.mkv")).join(" ");
        assert!(both.contains("-ar 48000 -ch_layout stereo -i tcp://127.0.0.1:5000"));
        assert!(both.contains("[1:a]aresample") && both.contains("-map [m] -map [s]"));
        s.mic = None;
        let only = capture_args(&s, Some(&system), SOFTWARE, Path::new("a.mkv")).join(" ");
        assert!(only.contains("[0:a]aresample") && !only.contains("[m]"));
    }

    #[test]
    fn sounds_are_lined_up_with_the_picture() {
        let log = "Input #0, dshow, from 'audio=x':\n  Duration: N/A, start: 100.250000, bitrate\n\
                   [Parsed_showinfo_7 @ 1] n:   0 pts:100500000 pts_time:100.5   duration:1";
        let both = Sources {
            mic: true,
            system: Some(100_000_000),
        };
        let found = leads(both, log);
        assert!((found[0] - 0.30).abs() < 1e-9 && (found[1] - 0.50).abs() < 1e-9);
        assert_eq!(leads(both, "nothing"), vec![0.0, 0.0]);
        let two = finish_args(&found).join(" ");
        assert!(two.contains("first_pts=14400") && two.contains("first_pts=24000"));
        assert!(two.contains("amix=inputs=2"));
        assert!(!finish_args(&[-0.1]).join(" ").contains("amix"));
        assert!(!finish_args(&[]).join(" ").contains("-c:a"));
    }

    /// Records the first screen (and first microphone) for a few seconds and
    /// checks the result. Needs a desktop session, so it is run by hand:
    /// `cargo test -p coupcat-desktop records_a_real_take -- --ignored`.
    #[test]
    #[ignore]
    fn records_a_real_take() {
        let found = devices().expect("devices");
        println!("{found:?}\nencoder: {}", encoder());
        let folder = std::env::temp_dir().join("coupcat-recorder-test");
        for screen in &found.screens {
            let settings = Settings {
                screen: screen.clone(),
                mic: found.mics.first().cloned(),
                system: found.system.is_some(),
                fps: 30,
                cursor: true,
            };
            let rec = start(&settings, &folder, "prueba").expect("start");
            std::thread::sleep(Duration::from_secs(5));
            let file = rec.stop().expect("stop");
            let info = concat_media::probe(&file).expect("probe");
            println!("{}: {info:?}", file.display());
            assert_eq!(file.extension().unwrap(), "mp4");
            fs::remove_file(file).unwrap();
        }
        let rec = start(
            &Settings {
                screen: found.screens[0].clone(),
                mic: None,
                system: true,
                fps: 30,
                cursor: true,
            },
            &folder,
            "cancelada",
        )
        .expect("start");
        rec.cancel();
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 0);
    }
}
