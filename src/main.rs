//! Zek Music — player de músicas leve para Linux.
//! Pasta = playlist, monitoramento automático, playlists personalizadas.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Frame, Layout, Margin, Pos2, Rect, RichText, Rounding,
    Sense, Stroke, Vec2,
};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rand::Rng;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder as SymDecoder, DecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(target_os = "linux")]
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ───────────────────────── Cores (azul + escuro) ─────────────────────────
const BG: Color32 = Color32::from_rgb(8, 12, 24);
const PANEL: Color32 = Color32::from_rgb(11, 17, 35);
const BTN: Color32 = Color32::from_rgb(24, 36, 70);
const BTN_HOVER: Color32 = Color32::from_rgb(34, 50, 96);
const RAIL: Color32 = Color32::from_rgb(36, 50, 92);
const ROW_HOVER: Color32 = Color32::from_rgb(18, 28, 56);
const SEL_BG: Color32 = Color32::from_rgb(22, 40, 86);
const ACCENT: Color32 = Color32::from_rgb(59, 130, 246);
const ACCENT_HI: Color32 = Color32::from_rgb(96, 165, 250);
const TEXT: Color32 = Color32::from_rgb(226, 233, 246);
const MUTED: Color32 = Color32::from_rgb(124, 140, 175);
const WARN: Color32 = Color32::from_rgb(251, 191, 100);

const AUDIO_EXT: &[&str] = &["mp3", "flac", "ogg", "oga", "wav", "m4a", "aac"];

const PALETTES: [([u8; 3], [u8; 3]); 8] = [
    ([37, 99, 235], [56, 189, 248]),
    ([79, 70, 229], [96, 165, 250]),
    ([14, 116, 144], [45, 212, 191]),
    ([30, 64, 175], [129, 140, 248]),
    ([2, 132, 199], [34, 211, 238]),
    ([67, 56, 202], [192, 132, 252]),
    ([29, 78, 216], [103, 232, 249]),
    ([12, 74, 110], [56, 189, 248]),
];

// ───────────────────────── Dados / configuração ─────────────────────────
#[derive(Serialize, Deserialize, Clone)]
enum PlaylistSource {
    /// Playlist automática: vem de uma pasta no disco.
    Folder(PathBuf),
    /// Playlist personalizada: lista de arquivos escolhidos pelo usuário.
    Custom(Vec<PathBuf>),
}

#[derive(Serialize, Deserialize, Clone)]
struct Playlist {
    id: u64,
    name: String,
    source: PlaylistSource,
    /// Playlist de sessão (ex.: arquivos abertos por "Abrir com o Zek Music"):
    /// existe só enquanto o programa está aberto e não é gravada no disco.
    #[serde(skip)]
    session: bool,
}

/// Onde o programa parou: playlist, música, posição e estado dos botões.
#[derive(Serialize, Deserialize, Clone, Default)]
struct SessionState {
    playlist: Option<u64>,
    track: Option<PathBuf>,
    pos: f32,
    was_playing: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
enum Repeat {
    Off,
    All,
    One,
}

impl Default for Repeat {
    fn default() -> Self {
        Repeat::Off
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Config {
    roots: Vec<PathBuf>,
    playlists: Vec<Playlist>, // a ordem do vetor é a ordem exibida
    next_id: u64,
    volume: f32,
    muted: bool,
    shuffle: bool,
    repeat: Repeat,
    /// Cor de destaque: índice na lista de paletas, ou uma cor escolhida à mão.
    accent_idx: u8,
    accent_custom: Option<[u8; 3]>,
    session: Option<SessionState>,
    /// Avisar quando sair versão nova no GitHub (uma consulta ao dia).
    check_updates: bool,
    last_update_check: i64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            roots: vec![], playlists: vec![], next_id: 0, volume: 0.8, muted: false, shuffle: false,
            repeat: Repeat::Off, accent_idx: 0, accent_custom: None, session: None,
            check_updates: true, last_update_check: 0,
        }
    }
}

#[derive(Clone, Default)]
struct Meta {
    title: String,
    artist: String,
    album: String,
    duration: f32,
    /// Número da faixa na etiqueta (ID3 etc.), quando existir.
    track: Option<u32>,
}

/// Build da Microsoft Store: compile com a variavel de ambiente STORE_BUILD=true.
fn is_store_build() -> bool {
    std::env::var("STORE_BUILD").ok().as_deref() == Some("true")
}

fn config_path() -> PathBuf {
    // Modo portátil: se existir "portable.txt" ao lado do programa, tudo fica na pasta "dados".
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("portable.txt").exists() {
                return dir.join("dados").join("config.json");
            }
        }
    }
    let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("zek-music");
    p.push("config.json");
    p
}

/// Pasta que guarda o registro de erros e as configurações (útil para o suporte).
fn data_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("portable.txt").exists() {
                return dir.join("dados");
            }
        }
    }
    let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("zek-music");
    p
}

fn load_config() -> Config {
    let p = config_path();
    match fs::read_to_string(&p) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(c) => return c,
            Err(e) => {
                log_warn(format!("config.json com problema ({e}); tentando a cópia de segurança"));
                let bak = p.with_extension("json.bak");
                if let Ok(s2) = fs::read_to_string(&bak) {
                    if let Ok(c) = serde_json::from_str(&s2) {
                        return c;
                    }
                }
                Config::default()
            }
        },
        Err(_) => Config::default(),
    }
}

// ───────────────────────── Utilidades ─────────────────────────
fn is_audio(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map_or(false, |e| AUDIO_EXT.contains(&e.to_ascii_lowercase().as_str()))
}

fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

fn folder_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string_lossy().to_string())
}

/// Remove sufixos como "(MP3_160K)" dos nomes baixados de conversores.
fn clean_title(s: &str) -> String {
    let t = s.trim();
    if t.ends_with(')') {
        if let Some(i) = t.rfind("(MP3_") {
            let cut = t[..i].trim();
            if !cut.is_empty() {
                return cut.to_string();
            }
        }
    }
    t.to_string()
}

fn fmt_time(t: f32) -> String {
    let s = t.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

fn fmt_total(t: f32) -> String {
    let m = (t / 60.0).round() as u32;
    if m >= 60 {
        format!("{} h {} min", m / 60, m % 60)
    } else {
        format!("{} min", m)
    }
}

fn initial(s: &str) -> String {
    s.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into())
}

fn hash_str(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222317;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn tile_colors(seed: &str) -> (Color32, Color32) {
    let p = PALETTES[(hash_str(seed) % 8) as usize];
    (Color32::from_rgb(p.0[0], p.0[1], p.0[2]), Color32::from_rgb(p.1[0], p.1[1], p.1[2]))
}

fn stroke(w: f32, c: Color32) -> Stroke {
    Stroke::new(w, c)
}

// ───────────────────────── Registro de erros ─────────────────────────
// No Windows o programa não tem terminal: quando algo dá errado, o motivo fica
// escrito neste arquivo. Dá para abrir a pasta pelo botão em "Ajustes".
static LOG_FILE: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();

fn log_path() -> PathBuf {
    data_dir().join("zek-music.log")
}

/// Segundos desde 1970 → "AAAA-MM-DD HH:MM:SS" (UTC, sem biblioteca externa).
fn fmt_stamp(secs: u64) -> String {
    let dias = (secs / 86400) as i64;
    let s = secs % 86400;
    let (h, mi, se) = (s / 3600, (s % 3600) / 60, s % 60);
    // calendário: dias desde 1970 → ano/mês/dia
    let d = dias + 719_468;
    let era = if d >= 0 { d } else { d - 146_096 } / 146_097;
    let doe = d - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", year, month, day, h, mi, se)
}

fn log_write(level: &str, msg: &str) {
    if let Some(m) = LOG_FILE.get() {
        if let Ok(mut g) = m.lock() {
            if let Some(f) = g.as_mut() {
                let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let _ = writeln!(f, "[{} {}] {}", fmt_stamp(secs), level, msg);
            }
        }
    }
}

fn log_info(m: impl std::fmt::Display) {
    log_write("INFO ", &m.to_string());
}
fn log_warn(m: impl std::fmt::Display) {
    log_write("AVISO", &m.to_string());
}
fn log_err(m: impl std::fmt::Display) {
    log_write("ERRO ", &m.to_string());
}

/// Últimas linhas do registro, para mostrar na janela de Ajustes.
fn log_tail(n: usize) -> Vec<String> {
    let txt = fs::read_to_string(log_path()).unwrap_or_default();
    txt.lines().rev().take(n).collect::<Vec<_>>().into_iter().rev().map(|s| s.to_string()).collect()
}

fn log_init() {
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    // não deixa o arquivo crescer sem limite: com mais de 512 KB, começa outro
    if let Ok(md) = fs::metadata(&path) {
        if md.len() > 512 * 1024 {
            let _ = fs::remove_file(&path);
        }
    }
    let file = fs::OpenOptions::new().create(true).append(true).open(&path).ok();
    let _ = LOG_FILE.set(Mutex::new(file));
    // qualquer "panic" (erro grave) também fica registrado
    let anterior = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_write("ERRO ", &format!("panic: {info}"));
        anterior(info);
    }));
    log_info(format!(
        "Zek Music v{} iniciado ({}, {})",
        env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH
    ));
}

// ───────────────────────── Ordem das faixas ─────────────────────────

/// Comparação "natural": "2 - música" vem antes de "10 - música".
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0usize, 0usize);
    while i < ab.len() && j < bb.len() {
        if ab[i].is_ascii_digit() && bb[j].is_ascii_digit() {
            let (si, sj) = (i, j);
            while i < ab.len() && ab[i].is_ascii_digit() {
                i += 1;
            }
            while j < bb.len() && bb[j].is_ascii_digit() {
                j += 1;
            }
            let (na, nb) = (&a[si..i], &b[sj..j]);
            let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
            let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)).then_with(|| na.len().cmp(&nb.len()));
            if ord != Ordering::Equal {
                return ord;
            }
        } else {
            let ord = ab[i].to_ascii_lowercase().cmp(&bb[j].to_ascii_lowercase());
            if ord != Ordering::Equal {
                return ord;
            }
            i += 1;
            j += 1;
        }
    }
    (ab.len() - i).cmp(&(bb.len() - j))
}

/// Ordena as faixas de uma pasta. Quando quase todas as músicas têm o número da
/// faixa nas etiquetas (e esse número não se repete), segue a ordem do álbum;
/// senão usa a ordem natural do nome do arquivo.
fn order_tracks(files: &mut Vec<PathBuf>, metas: &HashMap<PathBuf, Meta>) {
    let key = |p: &Path| p.to_string_lossy().to_lowercase();
    let com_numero = files.iter().filter(|p| metas.get(p.as_path()).and_then(|m| m.track).is_some()).count();
    let distintos: HashSet<u32> = files.iter().filter_map(|p| metas.get(p.as_path()).and_then(|m| m.track)).collect();
    if com_numero * 10 >= files.len() * 8 && distintos.len() == com_numero {
        let num = |p: &Path| metas.get(p).and_then(|m| m.track).unwrap_or(0);
        files.sort_by(|a, b| num(a).cmp(&num(b)).then_with(|| natural_cmp(&key(a), &key(b))));
    } else {
        files.sort_by(|a, b| natural_cmp(&key(a), &key(b)));
    }
}

// ───────────────────────── Cor de destaque escolhível ─────────────────────────
#[derive(Clone, Copy)]
struct Accent {
    base: Color32,
    hi: Color32,
}

impl Accent {
    fn from_cfg(cfg: &Config) -> Self {
        let base = match cfg.accent_custom {
            Some(c) => Color32::from_rgb(c[0], c[1], c[2]),
            None => {
                let p = PALETTES[(cfg.accent_idx as usize).min(PALETTES.len() - 1)];
                Color32::from_rgb(p.0[0], p.0[1], p.0[2])
            }
        };
        Accent { base, hi: lerp_color(base, Color32::WHITE, 0.18) }
    }
}

/// "v0.6.1" é mais novo que "v0.6.0"?
fn versao_mais_nova(tag: &str, atual: &str) -> bool {
    let nums = |s: &str| -> Vec<u32> {
        s.trim_start_matches('v').split('.').filter_map(|p| p.parse::<u32>().ok()).collect()
    };
    let (a, b) = (nums(tag), nums(atual));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// Versão mais nova no GitHub (etiqueta e endereço), se houver.
/// Falta de internet ou resposta estranha = None, sem incomodar.
fn fetch_latest_release() -> Option<(String, String)> {
    let resp = ureq::get("https://api.github.com/repos/aaaiiicerol/zek-music/releases/latest")
        .set("User-Agent", "zek-music")
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(8))
        .call()
        .ok()?;
    let body = resp.into_string().ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let tag = v.get("tag_name")?.as_str()?.to_string();
    let url = v.get("html_url").and_then(|u| u.as_str()).unwrap_or("https://github.com/aaaiiicerol/zek-music/releases").to_string();
    Some((tag, url))
}

/// Abre um link no navegador (usado no aviso de nova versão).
fn open_url(url: &str) {
    let r = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if r.is_err() {
        log_warn("não foi possível abrir o navegador");
    }
}

// ───────────────────────── Escaneamento de pastas ─────────────────────────
fn collect_audio(dir: &Path, out: &mut Vec<PathBuf>, depth: u8) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if is_hidden(&e.file_name()) {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            if depth < 4 {
                collect_audio(&p, out, depth + 1);
            }
        } else if is_audio(&p) {
            out.push(p);
        }
    }
}

/// A raiz está acessível? Uma pasta que existe mas está vazia (ponto de
/// montagem sem o disco montado) também conta como indisponível.
fn root_available(r: &Path) -> bool {
    if !r.exists() {
        return false;
    }
    match fs::read_dir(r) {
        Ok(mut rd) => rd.next().is_some(),
        Err(_) => false,
    }
}

/// Situação da pasta de uma playlist, para avisar o usuário sem perder nada.
#[derive(Clone, Copy, PartialEq)]
enum FolderStatus {
    Ok,
    Missing,
    Unmounted,
}

/// Cada subpasta (com músicas) de uma pasta raiz vira uma playlist.
/// Músicas soltas direto na raiz formam uma playlist com o nome da raiz.
/// A ordem das faixas é decidida depois, quando as etiquetas são lidas.
fn scan_roots(roots: &[PathBuf]) -> HashMap<PathBuf, Vec<PathBuf>> {
    let mut map = HashMap::new();
    for root in roots {
        if !root_available(root) {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        let mut loose = vec![];
        for e in rd.flatten() {
            if is_hidden(&e.file_name()) {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                let mut v = vec![];
                collect_audio(&p, &mut v, 0);
                if !v.is_empty() {
                    map.insert(p, v);
                }
            } else if is_audio(&p) {
                loose.push(p);
            }
        }
        if !loose.is_empty() {
            map.insert(root.clone(), loose);
        }
    }
    map
}

fn read_info(path: &Path, fallback: Option<f32>, want_cover: bool) -> (Meta, Option<Vec<u8>>) {
    let mut m = Meta { title: clean_title(&stem(path)), duration: fallback.unwrap_or(0.0), ..Default::default() };
    let mut cover = None;
    if let Ok(tf) = lofty::read_from_path(path) {
        let d = tf.properties().duration().as_secs_f32();
        if d > 0.0 {
            m.duration = d;
        }
        if let Some(tag) = tf.primary_tag().or_else(|| tf.first_tag()) {
            if let Some(t) = tag.title() {
                let t = clean_title(&t);
                if !t.is_empty() {
                    m.title = t;
                }
            }
            if let Some(a) = tag.artist() {
                m.artist = a.to_string();
            }
            if let Some(a) = tag.album() {
                m.album = a.to_string();
            }
            if let Some(n) = tag.track() {
                m.track = Some(n);
            }
            if want_cover {
                if let Some(pic) = tag.pictures().first() {
                    cover = Some(pic.data().to_vec());
                }
            }
        }
    }
    (m, cover)
}

/// Cor "vibrante" média da capa, usada no degradê da tela expandida.
fn dominant_color(img: &image::RgbaImage) -> Color32 {
    let (mut r, mut g, mut b, mut ws) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
    for p in img.pixels().step_by(41) {
        let (pr, pg, pb) = (p[0] as f32, p[1] as f32, p[2] as f32);
        let mx = pr.max(pg).max(pb);
        let mn = pr.min(pg).min(pb);
        let sat = (mx - mn) / 255.0;
        let lum = (pr + pg + pb) / 765.0;
        let w = (0.08 + sat) * (1.0 - (lum - 0.5).abs() * 1.4).max(0.05);
        r += pr * w;
        g += pg * w;
        b += pb * w;
        ws += w;
    }
    if ws <= 0.0 {
        return Color32::from_rgb(40, 70, 140);
    }
    let (r, g, b) = (r / ws, g / ws, b / ws);
    let k = 150.0 / r.max(g).max(b).max(1.0);
    Color32::from_rgb((r * k).min(255.0) as u8, (g * k).min(255.0) as u8, (b * k).min(255.0) as u8)
}

// ───────────────────────── Decodificação (com plano B) ─────────────────────────
/// Pula as etiquetas ID3v2 e vai direto para o primeiro quadro MPEG válido.
fn strip_tags(data: &[u8]) -> Vec<u8> {
    let mut start = 0usize;
    while data.len() >= start + 10 && &data[start..start + 3] == b"ID3" {
        let size = (((data[start + 6] & 0x7f) as usize) << 21)
            | (((data[start + 7] & 0x7f) as usize) << 14)
            | (((data[start + 8] & 0x7f) as usize) << 7)
            | ((data[start + 9] & 0x7f) as usize);
        let footer = if data[start + 5] & 0x10 != 0 { 10 } else { 0 };
        start += 10 + size + footer;
    }
    let mut i = start;
    while i + 3 < data.len() {
        let b1 = data[i + 1];
        let b2 = data[i + 2];
        if data[i] == 0xFF
            && (b1 & 0xE0) == 0xE0
            && ((b1 >> 3) & 3) != 1
            && ((b1 >> 1) & 3) != 0
            && (b2 >> 4) != 0xF
            && (b2 >> 4) != 0
            && ((b2 >> 2) & 3) != 3
        {
            return data[i..].to_vec();
        }
        i += 1;
    }
    data[start.min(data.len())..].to_vec()
}

const MP3_BR_V1: [u32; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
const MP3_BR_V2: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];

/// Se existir um quadro MPEG Layer III válido em `i`, devolve (tamanho_do_quadro, main_data_begin).
fn mp3_frame_at(d: &[u8], i: usize) -> Option<(usize, usize)> {
    if i + 4 > d.len() {
        return None;
    }
    let (b1, b2) = (d[i + 1], d[i + 2]);
    if d[i] != 0xFF || (b1 & 0xE0) != 0xE0 {
        return None;
    }
    let version = (b1 >> 3) & 3; // 3 = MPEG1, 2 = MPEG2, 0 = MPEG2.5
    let layer = (b1 >> 1) & 3; // 1 = Layer III
    if version == 1 || layer != 1 {
        return None;
    }
    let br_idx = (b2 >> 4) as usize;
    let sr_idx = ((b2 >> 2) & 3) as usize;
    if br_idx == 0 || br_idx == 15 || sr_idx == 3 {
        return None;
    }
    let mpeg1 = version == 3;
    let br = (if mpeg1 { MP3_BR_V1[br_idx] } else { MP3_BR_V2[br_idx] }) as usize * 1000;
    let sr_base = [44100usize, 48000, 32000][sr_idx];
    let sr = match version {
        3 => sr_base,
        2 => sr_base / 2,
        _ => sr_base / 4,
    };
    let pad = ((b2 >> 1) & 1) as usize;
    let len = if mpeg1 { 144 * br / sr + pad } else { 72 * br / sr + pad };
    let crc = if b1 & 1 == 0 { 2 } else { 0 };
    let off = i + 4 + crc;
    let mdb = if mpeg1 {
        if off + 1 >= d.len() {
            return None;
        }
        ((d[off] as usize) << 1) | ((d[off + 1] as usize) >> 7)
    } else {
        if off >= d.len() {
            return None;
        }
        d[off] as usize
    };
    Some((len, mdb))
}

/// Posições de quadros que não dependem de dados anteriores (bons pontos de partida).
fn mp3_clean_starts(d: &[u8], max: usize) -> Vec<usize> {
    let mut out = vec![];
    let mut i = 0usize;
    let mut frames = 0;
    while i + 4 < d.len() && out.len() < max && frames < 3000 {
        match mp3_frame_at(d, i) {
            Some((len, mdb)) if len > 4 => {
                let next_ok = i + len + 4 >= d.len() || mp3_frame_at(d, i + len).is_some();
                if next_ok {
                    if mdb == 0 {
                        out.push(i);
                    }
                    i += len;
                    frames += 1;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// Primeiro quadro MPEG válido (confirmado por mais dois quadros seguidos).
fn mp3_first_frame(d: &[u8]) -> Option<usize> {
    let limit = d.len().min(1 << 20);
    for i in 0..limit {
        if let Some((l1, _)) = mp3_frame_at(d, i) {
            if l1 > 4 {
                if let Some((l2, _)) = mp3_frame_at(d, i + l1) {
                    if l2 > 4 && mp3_frame_at(d, i + l1 + l2).is_some() {
                        return Some(i);
                    }
                }
            }
        }
    }
    None
}

/// Coloca 3 quadros de silêncio antes do áudio. Alguns MP3 começam num quadro que
/// depende de dados anteriores (que não existem); os quadros de silêncio preenchem
/// esse "reservatório" e o decodificador consegue começar.
fn mp3_with_silent_prefix(d: &[u8]) -> Option<Vec<u8>> {
    let i = mp3_first_frame(d)?;
    let (len, _) = mp3_frame_at(d, i)?;
    let pad = ((d[i + 2] >> 1) & 1) as usize;
    let base = len.checked_sub(pad)?;
    if base < 24 {
        return None;
    }
    // mesmo cabeçalho, sem CRC e sem padding
    let header = [d[i], d[i + 1] | 1, d[i + 2] & !0x02, d[i + 3]];
    let mut out = Vec::with_capacity(d.len() - i + base * 3);
    for _ in 0..3 {
        out.extend_from_slice(&header);
        out.resize(out.len() + base - 4, 0);
    }
    out.extend_from_slice(&d[i..]);
    Some(out)
}

/// Decodificador "tolerante": usa o symphonia direto e PULA os blocos com defeito
/// em vez de encerrar a música (o rodio para no primeiro erro).
struct TolerantSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn SymDecoder>,
    track_id: u32,
    buf: Vec<i16>,
    pos: usize,
    channels: u16,
    rate: u32,
    dur: Option<Duration>,
}

impl TolerantSource {
    fn new(data: Vec<u8>) -> Option<Self> {
        let mss = MediaSourceStream::new(Box::new(std::io::Cursor::new(data)), Default::default());
        let mut hint = Hint::new();
        hint.with_extension("mp3");
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .ok()?;
        let format = probed.format;
        let (track_id, params) = {
            let t = format.default_track()?;
            (t.id, t.codec_params.clone())
        };
        let decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).ok()?;
        let rate = params.sample_rate.unwrap_or(44100);
        let channels = params.channels.map(|c| c.count() as u16).unwrap_or(2);
        let dur = match (params.n_frames, params.sample_rate) {
            (Some(n), Some(r)) if r > 0 => Some(Duration::from_secs_f64(n as f64 / r as f64)),
            _ => None,
        };
        let mut s = Self { format, decoder, track_id, buf: vec![], pos: 0, channels, rate, dur };
        if !s.refill() {
            return None;
        }
        Some(s)
    }

    fn refill(&mut self) -> bool {
        loop {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(_) => return false,
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let spec = *decoded.spec();
                    let mut sb = SampleBuffer::<i16>::new(decoded.capacity() as u64, spec);
                    sb.copy_interleaved_ref(decoded);
                    self.channels = spec.channels.count() as u16;
                    self.rate = spec.rate;
                    self.buf.clear();
                    self.buf.extend_from_slice(sb.samples());
                    self.pos = 0;
                    if self.buf.is_empty() {
                        continue;
                    }
                    return true;
                }
                // bloco com defeito: ignora e segue para o próximo
                Err(SymError::DecodeError(_)) => continue,
                Err(_) => return false,
            }
        }
    }
}

impl Iterator for TolerantSource {
    type Item = i16;
    fn next(&mut self) -> Option<i16> {
        if self.pos >= self.buf.len() && !self.refill() {
            return None;
        }
        let v = self.buf[self.pos];
        self.pos += 1;
        Some(v)
    }
}

impl Source for TolerantSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        self.dur
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        match self.format.seek(
            SeekMode::Coarse,
            SeekTo::Time { time: Time::from(pos.as_secs_f64()), track_id: Some(self.track_id) },
        ) {
            Ok(_) => {
                self.decoder.reset();
                self.buf.clear();
                self.pos = 0;
                Ok(())
            }
            Err(_) => Err(rodio::source::SeekError::NotSupported { underlying_source: "TolerantSource" }),
        }
    }
}

fn make_source(path: &Path) -> Result<Box<dyn Source<Item = i16> + Send>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    match Decoder::new(std::io::BufReader::new(file)) {
        Ok(d) => Ok(Box::new(d)),
        Err(first) => {
            // Plano B: decodificador tolerante (pula blocos com defeito e continua tocando).
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            let cleaned = strip_tags(&data);
            if let Some(t) = TolerantSource::new(cleaned.clone()) {
                return Ok(Box::new(t));
            }
            if let Some(fixed) = mp3_with_silent_prefix(&cleaned) {
                if let Some(t) = TolerantSource::new(fixed) {
                    return Ok(Box::new(t));
                }
            }
            if let Some(t) = TolerantSource::new(data) {
                return Ok(Box::new(t));
            }
            Err(format!("{first}"))
        }
    }
}

// ───────────────────────── Desenho (gradientes, ícones, barras) ─────────────────────────
fn scale_color(c: Color32, k: f32) -> Color32 {
    Color32::from_rgb((c.r() as f32 * k) as u8, (c.g() as f32 * k) as u8, (c.b() as f32 * k) as u8)
}

fn gradient_rect_4(p: &egui::Painter, rect: Rect, tl: Color32, tr: Color32, br: Color32, bl: Color32) {
    let mut m = egui::Mesh::default();
    m.colored_vertex(rect.left_top(), tl);
    m.colored_vertex(rect.right_top(), tr);
    m.colored_vertex(rect.right_bottom(), br);
    m.colored_vertex(rect.left_bottom(), bl);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(m));
}

fn chevron(p: &egui::Painter, c: Pos2, up: bool, col: Color32) {
    let d = if up { -1.0 } else { 1.0 };
    let pts = vec![Pos2::new(c.x - 7.0, c.y - 3.0 * d), Pos2::new(c.x, c.y + 3.0 * d), Pos2::new(c.x + 7.0, c.y - 3.0 * d)];
    p.add(egui::Shape::line(pts, stroke(2.2, col)));
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn rounded_gradient(p: &egui::Painter, rect: Rect, r: f32, c1: Color32, c2: Color32) {
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [
        (rect.right() - r, rect.top() + r, -90.0_f32),
        (rect.right() - r, rect.bottom() - r, 0.0),
        (rect.left() + r, rect.bottom() - r, 90.0),
        (rect.left() + r, rect.top() + r, 180.0),
    ];
    let mut pts: Vec<Pos2> = vec![];
    for (cx, cy, start) in corners {
        for i in 0..=6 {
            let a = (start + i as f32 * 15.0).to_radians();
            pts.push(Pos2::new(cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    let col = |p: Pos2| -> Color32 {
        let t = ((p.x - rect.left()) / rect.width().max(1.0) + (p.y - rect.top()) / rect.height().max(1.0)) / 2.0;
        lerp_color(c1, c2, t.clamp(0.0, 1.0))
    };
    let mut mesh = egui::Mesh::default();
    let center = rect.center();
    mesh.colored_vertex(center, col(center));
    for q in &pts {
        mesh.colored_vertex(*q, col(*q));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + ((i + 1) % n));
    }
    p.add(egui::Shape::mesh(mesh));
}

fn gradient_rect_v(p: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut m = egui::Mesh::default();
    m.colored_vertex(rect.left_top(), top);
    m.colored_vertex(rect.right_top(), top);
    m.colored_vertex(rect.right_bottom(), bottom);
    m.colored_vertex(rect.left_bottom(), bottom);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(m));
}

fn equalizer(p: &egui::Painter, center: Pos2, t: f64, animate: bool, accent_hi: Color32) {
    for i in 0..3 {
        let h = if animate { 4.0 + 9.0 * (((t * 6.0 + i as f64 * 1.7).sin() * 0.5 + 0.5) as f32) } else { 5.0 };
        let x = center.x - 6.0 + i as f32 * 6.0;
        p.rect_filled(
            Rect::from_min_max(Pos2::new(x - 1.6, center.y + 7.0 - h), Pos2::new(x + 1.6, center.y + 7.0)),
            1.0,
            accent_hi,
        );
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Icon {
    Play,
    Pause,
    Next,
    Prev,
    Shuffle,
    Repeat,
    RepeatOne,
    Volume,
    Muted,
}

fn arrow_head(p: &egui::Painter, tip: Pos2, s: f32, col: Color32) {
    p.add(egui::Shape::convex_polygon(
        vec![Pos2::new(tip.x + 0.7 * s, tip.y), Pos2::new(tip.x - 0.3 * s, tip.y - 0.7 * s), Pos2::new(tip.x - 0.3 * s, tip.y + 0.7 * s)],
        col,
        Stroke::NONE,
    ));
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, size: f32, accent: bool, active: bool, acc: Accent) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let hov = resp.hovered();
    let toggle_like = matches!(icon, Icon::Shuffle | Icon::Repeat | Icon::RepeatOne | Icon::Volume | Icon::Muted);
    let col = if accent {
        Color32::WHITE
    } else if active {
        acc.hi
    } else if hov {
        Color32::WHITE
    } else if toggle_like {
        MUTED
    } else {
        TEXT
    };
    let p = ui.painter();
    if accent {
        p.circle_filled(rect.center(), size / 2.0, if hov { acc.hi } else { acc.base });
    }
    let c = rect.center();
    let s = size * 0.2;
    let poly = |pts: Vec<Pos2>| egui::Shape::convex_polygon(pts, col, Stroke::NONE);
    match icon {
        Icon::Play => {
            p.add(poly(vec![Pos2::new(c.x - 0.8 * s, c.y - 1.1 * s), Pos2::new(c.x - 0.8 * s, c.y + 1.1 * s), Pos2::new(c.x + 1.1 * s, c.y)]));
        }
        Icon::Pause => {
            p.rect_filled(Rect::from_min_max(Pos2::new(c.x - 0.9 * s, c.y - 1.1 * s), Pos2::new(c.x - 0.25 * s, c.y + 1.1 * s)), 1.0, col);
            p.rect_filled(Rect::from_min_max(Pos2::new(c.x + 0.25 * s, c.y - 1.1 * s), Pos2::new(c.x + 0.9 * s, c.y + 1.1 * s)), 1.0, col);
        }
        Icon::Next => {
            p.add(poly(vec![Pos2::new(c.x - 1.1 * s, c.y - s), Pos2::new(c.x - 1.1 * s, c.y + s), Pos2::new(c.x + 0.5 * s, c.y)]));
            p.rect_filled(Rect::from_min_max(Pos2::new(c.x + 0.8 * s, c.y - s), Pos2::new(c.x + 1.3 * s, c.y + s)), 1.0, col);
        }
        Icon::Prev => {
            p.add(poly(vec![Pos2::new(c.x + 1.1 * s, c.y - s), Pos2::new(c.x + 1.1 * s, c.y + s), Pos2::new(c.x - 0.5 * s, c.y)]));
            p.rect_filled(Rect::from_min_max(Pos2::new(c.x - 1.3 * s, c.y - s), Pos2::new(c.x - 0.8 * s, c.y + s)), 1.0, col);
        }
        Icon::Shuffle => {
            let u = size * 0.17;
            let st = stroke(1.8, col);
            p.line_segment([Pos2::new(c.x - 1.6 * u, c.y - 1.0 * u), Pos2::new(c.x + 1.2 * u, c.y + 1.0 * u)], st);
            p.line_segment([Pos2::new(c.x - 1.6 * u, c.y + 1.0 * u), Pos2::new(c.x + 1.2 * u, c.y - 1.0 * u)], st);
            arrow_head(p, Pos2::new(c.x + 1.4 * u, c.y + 1.0 * u), u * 1.2, col);
            arrow_head(p, Pos2::new(c.x + 1.4 * u, c.y - 1.0 * u), u * 1.2, col);
        }
        Icon::Repeat | Icon::RepeatOne => {
            let u = size * 0.17;
            p.rect_stroke(Rect::from_center_size(c, Vec2::new(3.6 * u, 2.0 * u)), 0.9 * u, stroke(1.8, col));
            arrow_head(p, Pos2::new(c.x + 0.5 * u, c.y - 1.0 * u), u * 1.3, col);
            if icon == Icon::RepeatOne {
                p.text(c, Align2::CENTER_CENTER, "1", FontId::proportional(size * 0.34), col);
            }
        }
        Icon::Volume | Icon::Muted => {
            let u = size * 0.15;
            p.rect_filled(Rect::from_min_max(Pos2::new(c.x - 2.0 * u, c.y - 0.8 * u), Pos2::new(c.x - 0.9 * u, c.y + 0.8 * u)), 1.0, col);
            p.add(poly(vec![
                Pos2::new(c.x - 0.9 * u, c.y - 0.8 * u),
                Pos2::new(c.x + 0.4 * u, c.y - 1.7 * u),
                Pos2::new(c.x + 0.4 * u, c.y + 1.7 * u),
                Pos2::new(c.x - 0.9 * u, c.y + 0.8 * u),
            ]));
            if icon == Icon::Volume {
                for r in [1.3_f32, 2.2] {
                    let pts: Vec<Pos2> = (0..=8)
                        .map(|i| {
                            let a = (-45.0_f32 + i as f32 * 11.25).to_radians();
                            Pos2::new(c.x + 0.4 * u + r * u * a.cos(), c.y + r * u * a.sin())
                        })
                        .collect();
                    p.add(egui::Shape::line(pts, stroke(1.6, col)));
                }
            } else {
                let st = stroke(1.8, col);
                p.line_segment([Pos2::new(c.x + 1.2 * u, c.y - 1.0 * u), Pos2::new(c.x + 2.8 * u, c.y + 1.0 * u)], st);
                p.line_segment([Pos2::new(c.x + 1.2 * u, c.y + 1.0 * u), Pos2::new(c.x + 2.8 * u, c.y - 1.0 * u)], st);
            }
        }
    }
    if active && toggle_like {
        p.circle_filled(Pos2::new(c.x, rect.bottom() - 1.5), 1.8, acc.hi);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn arrow_btn(ui: &mut egui::Ui, rect: Rect, up: bool, id: egui::Id) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click());
    let c = rect.center();
    let s = 4.5;
    let col = if resp.hovered() { Color32::WHITE } else { MUTED };
    let pts = if up {
        vec![Pos2::new(c.x - s, c.y + s * 0.6), Pos2::new(c.x + s, c.y + s * 0.6), Pos2::new(c.x, c.y - s * 0.8)]
    } else {
        vec![Pos2::new(c.x - s, c.y - s * 0.6), Pos2::new(c.x + s, c.y - s * 0.6), Pos2::new(c.x, c.y + s * 0.8)]
    };
    ui.painter().add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

struct BarOut {
    changed: bool,
    released: bool,
    dragging: bool,
}

/// Barra de progresso/volume desenhada à mão (clique e arraste).
fn bar(ui: &mut egui::Ui, value: &mut f32, max: f32, width: f32, enabled: bool, acc: Accent) -> BarOut {
    let sense = if enabled { Sense::click_and_drag() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 16.0), sense);
    let mut out = BarOut { changed: false, released: false, dragging: false };
    if enabled {
        if resp.dragged() || resp.drag_started() || resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let t = ((pos.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
                *value = t * max;
                out.changed = true;
            }
        }
        out.dragging = resp.dragged();
        out.released = resp.drag_stopped() || resp.clicked();
    }
    let hot = enabled && (resp.hovered() || resp.dragged());
    let frac = if max > 0.0 { (*value / max).clamp(0.0, 1.0) } else { 0.0 };
    let cy = rect.center().y;
    let rail_h = if hot { 6.0 } else { 4.0 };
    let p = ui.painter();
    let rail = Rect::from_min_max(Pos2::new(rect.left(), cy - rail_h / 2.0), Pos2::new(rect.right(), cy + rail_h / 2.0));
    p.rect_filled(rail, rail_h / 2.0, RAIL);
    let fill = Rect::from_min_max(rail.min, Pos2::new(rail.left() + rail.width() * frac, rail.max.y));
    if enabled {
        p.rect_filled(fill, rail_h / 2.0, if hot { acc.hi } else { acc.base });
    }
    if hot {
        p.circle_filled(Pos2::new(fill.right(), cy), 6.5, Color32::WHITE);
    }
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    out
}

fn columns(rect: Rect) -> (f32, f32, f32) {
    let title_x = rect.left() + 60.0;
    let usable = (rect.width() - 60.0 - 120.0).max(100.0);
    let artist_x = title_x + usable * 0.58;
    let dur_x = rect.right() - 28.0;
    (title_x, artist_x, dur_x)
}

// ───────────────────────── Estado do app ─────────────────────────

// ───────────────────────── Motor de áudio (thread própria) ─────────────────────────
// Roda separado da janela: continua tocando e trocando de música mesmo com o
// programa em segundo plano (ex.: dentro de um jogo) e recebe as teclas de mídia.
enum Cmd {
    PlayQueue { queue: Vec<PathBuf>, index: usize, playlist: Option<u64> },
    /// Retomar a última sessão: mesma fila, posição e se deve voltar a tocar.
    Restore { queue: Vec<PathBuf>, index: usize, pos: f32, play: bool, playlist: Option<u64> },
    Play,
    Pause,
    Toggle,
    Next,
    Prev,
    Stop,
    Seek(f32),
    Volume(f32),
    Shuffle(bool),
    Repeat(Repeat),
}

#[derive(Default)]
struct Shared {
    now: Option<Meta>,
    cover: Option<Vec<u8>>,
    cover_gen: u64,
    active: bool,
    paused: bool,
    pos: f32,
    cur_path: Option<PathBuf>,
    queue_pl: Option<u64>,
    has_queue: bool,
    status: String,
}

#[derive(Default, Clone)]
struct Snap {
    now: Option<Meta>,
    active: bool,
    paused: bool,
    pos: f32,
    cur_path: Option<PathBuf>,
    queue_pl: Option<u64>,
    has_queue: bool,
    status: String,
}

#[cfg(target_os = "linux")]
fn make_controls(tx: Sender<Cmd>) -> Option<MediaControls> {
    let cfg = PlatformConfig { dbus_name: "zek_music", display_name: "Zek Music", hwnd: None };
    let mut c = MediaControls::new(cfg).ok()?;
    let r = c.attach(move |ev: MediaControlEvent| {
        let cmd = match ev {
            MediaControlEvent::Play => Cmd::Play,
            MediaControlEvent::Pause => Cmd::Pause,
            MediaControlEvent::Toggle => Cmd::Toggle,
            MediaControlEvent::Next => Cmd::Next,
            MediaControlEvent::Previous => Cmd::Prev,
            MediaControlEvent::Stop => Cmd::Stop,
            MediaControlEvent::SetPosition(MediaPosition(d)) => Cmd::Seek(d.as_secs_f32()),
            _ => return,
        };
        let _ = tx.send(cmd);
    });
    if r.is_err() {
        return None;
    }
    Some(c)
}

/// Nome do dispositivo de saída padrão (usado no Windows para perceber a troca de fone/caixa).
#[cfg(windows)]
fn default_output_name() -> Option<String> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    rodio::cpal::default_host().default_output_device().and_then(|d| d.name().ok())
}
#[cfg(not(windows))]
fn default_output_name() -> Option<String> {
    None
}

struct Engine {
    rx: Receiver<Cmd>,
    shared: Arc<Mutex<Shared>>,
    ctx: egui::Context,
    stream: Option<(OutputStream, OutputStreamHandle)>,
    sink: Option<Sink>,
    queue: Vec<PathBuf>,
    qindex: usize,
    queue_pl: Option<u64>,
    paused: bool,
    shuffle: bool,
    repeat: Repeat,
    volume: f32,
    last_pos: f32,
    device_name: Option<String>,
    #[allow(dead_code)]
    last_device_check: Instant,
    stall_pos: f32,
    stall_since: Option<Instant>,
    now: Option<Meta>,
    #[cfg(target_os = "linux")]
    controls: Option<MediaControls>,
}

impl Engine {
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    fn new(
        rx: Receiver<Cmd>,
        tx: Sender<Cmd>,
        shared: Arc<Mutex<Shared>>,
        ctx: egui::Context,
        volume: f32,
        shuffle: bool,
        repeat: Repeat,
    ) -> Self {
        Self {
            rx,
            shared,
            ctx,
            stream: OutputStream::try_default().ok(),
            sink: None,
            queue: vec![],
            qindex: 0,
            queue_pl: None,
            paused: false,
            shuffle,
            repeat,
            volume,
            last_pos: 0.0,
            device_name: default_output_name(),
            last_device_check: Instant::now(),
            stall_pos: -1.0,
            stall_since: None,
            now: None,
            #[cfg(target_os = "linux")]
            controls: make_controls(tx),
        }
    }

    fn run(mut self) {
        loop {
            match self.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(cmd) => self.handle(cmd),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            self.tick();
        }
    }

    /// Reabre a saída de áudio e continua a música do mesmo ponto
    /// (ao trocar de fone/caixa de som ou quando o dispositivo some).
    fn reopen_output(&mut self) {
        let pos = self.sink.as_ref().map_or(0.0, |s| s.get_pos().as_secs_f32());
        let had_sink = self.sink.is_some();
        let paused = self.paused;
        if let Some(old) = self.sink.take() {
            old.stop();
        }
        self.stream = None;
        self.stream = OutputStream::try_default().ok();
        self.stall_since = None;
        self.stall_pos = -1.0;
        if self.stream.is_none() {
            log_err("nenhuma saída de áudio encontrada");
            self.set_status("Nenhuma saída de áudio encontrada. Conecte um dispositivo e toque de novo.".into());
            self.publish_full();
            self.mpris_update(false);
            return;
        }
        if had_sink {
            if let Some(path) = self.queue.get(self.qindex).cloned() {
                if let (Ok(source), Some((_, handle))) = (make_source(&path), &self.stream) {
                    if let Ok(sink) = Sink::try_new(handle) {
                        sink.set_volume(self.volume);
                        sink.append(source);
                        if pos > 0.5 {
                            let _ = sink.try_seek(Duration::from_secs_f32(pos));
                        }
                        if paused {
                            sink.pause();
                        }
                        self.sink = Some(sink);
                    }
                }
            }
        }
        self.set_status(String::new());
        self.publish_full();
        self.mpris_update(false);
    }

    fn check_output(&mut self) {
        #[cfg(windows)]
        {
            if self.last_device_check.elapsed() >= Duration::from_millis(1500) {
                self.last_device_check = Instant::now();
                let nome = default_output_name();
                if nome.is_some() && nome != self.device_name {
                    self.device_name = nome;
                    self.reopen_output();
                    return;
                }
            }
        }
        // Tempo parado por mais de 3 s com a música "tocando" (ex.: fone desconectado)
        let mut stalled = false;
        if let Some(s) = &self.sink {
            if !self.paused && !s.empty() {
                let pos = s.get_pos().as_secs_f32();
                if (pos - self.stall_pos).abs() < 0.001 {
                    let t = *self.stall_since.get_or_insert_with(Instant::now);
                    stalled = t.elapsed() > Duration::from_secs(3);
                } else {
                    self.stall_pos = pos;
                    self.stall_since = None;
                }
            } else {
                self.stall_since = None;
            }
        } else {
            self.stall_since = None;
        }
        if stalled {
            self.reopen_output();
        }
    }

    fn tick(&mut self) {
        self.check_output();
        let ended = !self.paused && self.sink.as_ref().map_or(false, |s| s.empty());
        if ended {
            let early = self.last_pos < 1.5 && self.now.as_ref().map_or(false, |n| n.duration > 5.0);
            let nome = self.now.as_ref().map(|n| n.title.clone()).unwrap_or_default();
            self.advance(true);
            if early {
                self.set_status(format!("\"{}\" parou logo no começo (arquivo com defeito?). Pulei para a próxima.", nome));
                self.publish_full();
            }
        } else {
            self.publish_pos();
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::PlayQueue { queue, index, playlist } => {
                self.queue = queue;
                self.queue_pl = playlist;
                self.play_index(index);
            }
            Cmd::Restore { queue, index, pos, play, playlist } => {
                self.queue = queue;
                self.queue_pl = playlist;
                self.qindex = index;
                if self.play_index(index) {
                    if pos > 1.0 {
                        if let Some(s) = &self.sink {
                            let _ = s.try_seek(Duration::from_secs_f32(pos));
                        }
                    }
                    if !play {
                        if let Some(s) = &self.sink {
                            s.pause();
                        }
                        self.paused = true;
                    }
                    self.set_status(if play {
                        "Retomando de onde você parou".into()
                    } else {
                        "Última música carregada — aperte play".into()
                    });
                    self.publish_full();
                    self.mpris_update(true);
                }
            }
            Cmd::Play => {
                if self.sink.is_none() || self.paused {
                    self.toggle();
                }
            }
            Cmd::Pause => {
                if self.sink.is_some() && !self.paused {
                    self.toggle();
                }
            }
            Cmd::Toggle => self.toggle(),
            Cmd::Next => self.advance(false),
            Cmd::Prev => self.prev(),
            Cmd::Stop => self.stop(),
            Cmd::Seek(t) => {
                if let Some(s) = &self.sink {
                    let _ = s.try_seek(Duration::from_secs_f32(t.max(0.0)));
                }
                self.publish_full();
                self.mpris_update(false);
            }
            Cmd::Volume(v) => {
                self.volume = v;
                if let Some(s) = &self.sink {
                    s.set_volume(v);
                }
            }
            Cmd::Shuffle(b) => self.shuffle = b,
            Cmd::Repeat(r) => self.repeat = r,
        }
    }

    fn publish_full(&self) {
        let pos = self.sink.as_ref().map_or(0.0, |s| s.get_pos().as_secs_f32());
        {
            let mut g = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            g.now = self.now.clone();
            g.active = self.sink.is_some();
            g.paused = self.paused;
            g.pos = pos;
            g.cur_path = if self.sink.is_some() { self.queue.get(self.qindex).cloned() } else { None };
            g.queue_pl = self.queue_pl;
            g.has_queue = !self.queue.is_empty();
        }
        self.ctx.request_repaint();
    }

    fn publish_pos(&mut self) {
        if let Some(s) = &self.sink {
            let pos = s.get_pos().as_secs_f32();
            self.last_pos = pos;
            let mut g = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            g.pos = pos;
        }
    }

    fn set_status(&self, s: String) {
        let mut g = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        g.status = s;
    }

    fn set_cover(&self, c: Option<Vec<u8>>) {
        let mut g = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        g.cover = c;
        g.cover_gen += 1;
    }

    #[cfg(not(target_os = "linux"))]
    fn mpris_update(&mut self, _meta_changed: bool) {}

    #[cfg(target_os = "linux")]
    fn mpris_update(&mut self, meta_changed: bool) {
        let pos = self.sink.as_ref().map_or(0.0, |s| s.get_pos().as_secs_f32());
        let stopped = self.sink.is_none();
        let paused = self.paused;
        if let Some(c) = self.controls.as_mut() {
            if meta_changed {
                if let Some(n) = &self.now {
                    let _ = c.set_metadata(MediaMetadata {
                        title: Some(n.title.as_str()),
                        artist: Some(n.artist.as_str()),
                        album: Some(n.album.as_str()),
                        cover_url: None,
                        duration: Some(Duration::from_secs_f32(n.duration.max(0.0))),
                    });
                }
            }
            let progress = Some(MediaPosition(Duration::from_secs_f32(pos.max(0.0))));
            let pb = if stopped {
                MediaPlayback::Stopped
            } else if paused {
                MediaPlayback::Paused { progress }
            } else {
                MediaPlayback::Playing { progress }
            };
            let _ = c.set_playback(pb);
        }
    }

    fn play_index(&mut self, idx: usize) -> bool {
        if idx >= self.queue.len() {
            return false;
        }
        let path = self.queue[idx].clone();
        self.qindex = idx;
        if self.stream.is_none() {
            self.stream = OutputStream::try_default().ok();
        }
        let Some((_, handle)) = &self.stream else {
            self.set_status("Nenhuma saída de áudio encontrada.".into());
            self.publish_full();
            return false;
        };
        let source = match make_source(&path) {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("não foi possível tocar {}: {e}", path.display());
                log_err(&msg);
                self.set_status(format!("Não foi possível tocar \"{}\": {}", clean_title(&stem(&path)), e));
                self.publish_full();
                return false;
            }
        };
        let src_dur = source.total_duration().map(|d| d.as_secs_f32());
        let sink = match Sink::try_new(handle) {
            Ok(s) => s,
            Err(_) => {
                self.set_status("Erro ao iniciar o áudio.".into());
                self.publish_full();
                return false;
            }
        };
        sink.set_volume(self.volume);
        sink.append(source);
        if let Some(old) = self.sink.take() {
            old.stop();
        }
        self.sink = Some(sink);
        let (m, cover) = read_info(&path, src_dur, true);
        self.set_cover(cover);
        self.now = Some(m);
        self.last_pos = 0.0;
        self.paused = false;
        self.set_status(String::new());
        self.publish_full();
        self.mpris_update(true);
        true
    }

    fn stop(&mut self) {
        if let Some(s) = self.sink.take() {
            s.stop();
        }
        self.now = None;
        self.paused = false;
        self.set_cover(None);
        self.publish_full();
        self.mpris_update(true);
    }

    fn toggle(&mut self) {
        if let Some(s) = &self.sink {
            if self.paused {
                s.play();
            } else {
                s.pause();
            }
            self.paused = !self.paused;
            self.publish_full();
            self.mpris_update(false);
        } else if !self.queue.is_empty() {
            let i = self.qindex.min(self.queue.len() - 1);
            self.play_index(i);
        }
    }

    fn pick_next(&self, cur: usize, auto: bool, allow_one: bool) -> Option<usize> {
        let n = self.queue.len();
        if n == 0 {
            return None;
        }
        if auto && allow_one && self.repeat == Repeat::One {
            return Some(cur);
        }
        if self.shuffle && n > 1 {
            let mut rng = rand::thread_rng();
            let mut r = cur;
            while r == cur {
                r = rng.gen_range(0..n);
            }
            return Some(r);
        }
        if cur + 1 < n {
            Some(cur + 1)
        } else if self.repeat == Repeat::All || !auto {
            Some(0)
        } else {
            None
        }
    }

    fn advance(&mut self, auto: bool) {
        let n = self.queue.len();
        if n == 0 {
            return;
        }
        let mut cur = self.qindex;
        let mut first = true;
        for _ in 0..n.min(10) {
            let Some(next) = self.pick_next(cur, auto || !first, first) else {
                self.stop();
                return;
            };
            if self.play_index(next) {
                return;
            }
            cur = next;
            first = false;
        }
        self.stop();
    }

    fn prev(&mut self) {
        let n = self.queue.len();
        if n == 0 {
            return;
        }
        let pos = self.sink.as_ref().map_or(0.0, |s| s.get_pos().as_secs_f32());
        if pos > 3.0 {
            if let Some(s) = &self.sink {
                let _ = s.try_seek(Duration::ZERO);
            }
            self.publish_full();
        } else {
            let i = if self.qindex == 0 { n - 1 } else { self.qindex - 1 };
            self.play_index(i);
        }
    }
}

enum ScanMsg {
    Folders(HashMap<PathBuf, Vec<PathBuf>>),
    Metas(Vec<(PathBuf, Meta)>),
    Done,
}

enum Act {
    Select(u64),
    StartRename(u64),
    CommitRename,
    CancelRename,
    MoveUp(usize),
    MoveDown(usize),
    Delete(u64),
    ResetName(u64),
    NewPlaylist,
    NewWith(PathBuf),
    AddFolders,
    RemoveRoot(usize),
    Refresh,
    PlayAt(usize),
    PlayAll,
    AddTo(u64, PathBuf),
    RemoveTrack(usize),
    AddFiles,
    /// Mover uma playlist (arrastando) para outra posição.
    MovePlaylistTo(u64, usize),
    SetAccent(u8),
    SetAccentCustom(Option<[u8; 3]>),
    SetCheckUpdates(bool),
    CheckUpdates,
    OpenDataDir,
    Toggle,
    Next,
    Prev,
    Seek(f32),
    VolChanged,
}

struct ZekApp {
    ctx: egui::Context,
    cfg: Config,
    tracks: HashMap<PathBuf, Vec<PathBuf>>,
    meta: HashMap<PathBuf, Meta>,
    selected: Option<u64>,
    renaming: Option<(u64, String)>,
    rename_focus: bool,
    search: String,
    show_folders: bool,

    cmd_tx: Sender<Cmd>,
    shared: Arc<Mutex<Shared>>,
    snap: Snap,
    cover: Option<egui::TextureHandle>,
    cover_color: Option<Color32>,
    cover_gen: u64,
    expanded: bool,
    bar_top: f32,
    shuffle: bool,
    repeat: Repeat,
    muted: bool,
    seek_val: f32,
    seeking: bool,

    watcher: Option<RecommendedWatcher>,
    fs_tx: Sender<()>,
    fs_rx: Receiver<()>,
    dirty_since: Option<Instant>,
    scan_rx: Option<Receiver<ScanMsg>>,
    /// Cor de destaque escolhida em Ajustes (aplica em todo o programa).
    accent: Accent,
    /// Quais pastas raiz estão acessíveis agora (disco montado ou não).
    root_ok: HashMap<PathBuf, bool>,
    /// Onde o programa parou na última vez, para retomar ao abrir.
    restore: Option<SessionState>,
    /// Capa decodificada fora da tela, para não travar ao trocar de música.
    cover_tx: Sender<CoverMsg>,
    cover_rx: Receiver<CoverMsg>,
    /// Aviso de nova versão (consulta o GitHub).
    upd_tx: Sender<(String, String)>,
    upd_rx: Receiver<(String, String)>,
    update_info: Option<(String, String)>,
    upd_dismissed: bool,
    show_settings: bool,
    /// Playlist sendo arrastada na lista (para reordenar ao soltar).
    drag_pl: Option<u64>,
    drag_target: Option<usize>,
    last_session_save: Instant,
}

/// Capa já decodificada, pronta para virar textura (decodificação fora da tela).
struct CoverMsg {
    gen: u64,
    img: egui::ColorImage,
    color: Color32,
}

/// Decodifica a capa fora da thread da janela: capas grandes demoram e
/// faziam a interface engasgar na troca de música.
fn decode_cover(bytes: &[u8]) -> Option<(egui::ColorImage, Color32)> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = img.thumbnail(320, 320).to_rgba8();
    let color = dominant_color(&img);
    let size = [img.width() as usize, img.height() as usize];
    let ci = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some((ci, color))
}

impl ZekApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let cfg = load_config();
        let accent = Accent::from_cfg(&cfg);
        setup_style(&cc.egui_ctx, accent);
        let (fs_tx, fs_rx) = mpsc::channel();
        let selected = cfg.playlists.first().map(|p| p.id);
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (vol0, shuf0, rep0, mud0, mut restaurar) =
            (cfg.volume, cfg.shuffle, cfg.repeat, cfg.muted, cfg.session.clone());
        let shared = Arc::new(Mutex::new(Shared::default()));
        {
            let (sh, cx, tx, vol) = (shared.clone(), cc.egui_ctx.clone(), cmd_tx.clone(), vol0);
            std::thread::spawn(move || {
                Engine::new(cmd_rx, tx, sh, cx, vol, shuf0, rep0).run();
            });
        }
        // "Abrir com o Zek Music": duplo clique num .mp3 abre o programa com ele
        let abertos: Vec<PathBuf> = std::env::args_os()
            .skip(1)
            .map(PathBuf::from)
            .filter(|p| is_audio(p) && p.exists())
            .collect();
        if !abertos.is_empty() {
            log_info(format!("programa aberto com {} arquivo(s) para tocar", abertos.len()));
        }
        let (cover_tx, cover_rx) = mpsc::channel();
        let (upd_tx, upd_rx) = mpsc::channel();
        if mud0 {
            // começa silenciado se estava silenciado ao fechar
            let _ = cmd_tx.send(Cmd::Volume(0.0));
        }
        // arquivos abertos por "Abrir com" têm prioridade sobre a última sessão
        if !abertos.is_empty() {
            restaurar = None;
        }
        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            accent,
            root_ok: HashMap::new(),
            restore: restaurar,
            cover_tx,
            cover_rx,
            upd_tx,
            upd_rx,
            update_info: None,
            upd_dismissed: false,
            show_settings: false,
            drag_pl: None,
            drag_target: None,
            last_session_save: Instant::now(),
            cfg,
            tracks: HashMap::new(),
            meta: HashMap::new(),
            selected,
            renaming: None,
            rename_focus: false,
            search: String::new(),
            show_folders: false,
            cmd_tx,
            shared,
            snap: Snap::default(),
            cover: None,
            cover_color: None,
            cover_gen: 0,
            expanded: false,
            bar_top: 0.0,
            shuffle: shuf0,
            repeat: rep0,
            muted: mud0,
            seek_val: 0.0,
            seeking: false,
            watcher: None,
            fs_tx,
            fs_rx,
            dirty_since: None,
            scan_rx: None,
        };
        app.refresh_root_ok();
        if !abertos.is_empty() {
            app.restore = None;
            app.open_files(abertos);
        }
        if !app.cfg.roots.is_empty() {
            app.restart_watcher();
            app.start_scan();
        }
        // uma consulta ao GitHub por dia, sem segurar a abertura do programa
        if app.cfg.check_updates && !is_store_build() && app.due_update_check() {
            app.spawn_update_check();
        }
        app
    }

    /// Guarda o estado atual no disco: playlists, volume, onde parou.
    /// As playlists de sessão (arquivos abertos) ficam de fora.
    fn save_config(&mut self) {
        let p = config_path();
        if let Some(dir) = p.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let sessoes: Vec<Playlist> = self.cfg.playlists.iter().filter(|p| p.session).cloned().collect();
        self.cfg.playlists.retain(|p| !p.session);
        self.cfg.muted = self.muted;
        self.cfg.shuffle = self.shuffle;
        self.cfg.repeat = self.repeat;
        self.cfg.session = Some(SessionState {
            playlist: self.selected,
            track: self.snap.cur_path.clone(),
            pos: self.snap.pos,
            was_playing: self.snap.active && !self.snap.paused,
        });
        // cópia de segurança: se o arquivo estragar, as playlists não se perdem
        let bak = p.with_extension("json.bak");
        let _ = fs::remove_file(&bak);
        let _ = fs::rename(&p, &bak);
        let r = serde_json::to_string_pretty(&self.cfg).ok().and_then(|s| fs::write(&p, s).ok());
        if r.is_none() {
            log_warn(format!("não foi possível salvar as configurações em {}", p.display()));
        }
        if !sessoes.is_empty() {
            self.cfg.playlists.extend(sessoes);
        }
    }

    /// Quais pastas raiz estão montadas agora (para avisar sem apagar nada).
    fn refresh_root_ok(&mut self) {
        self.root_ok = self.cfg.roots.iter().map(|r| (r.clone(), root_available(r))).collect();
    }

    fn estado_pasta(&self, path: &Path) -> FolderStatus {
        match self.cfg.roots.iter().find(|r| path.starts_with(r)) {
            None => FolderStatus::Missing,
            Some(raiz) => {
                if self.root_ok.get(raiz).copied().unwrap_or(true) {
                    if path.exists() {
                        FolderStatus::Ok
                    } else {
                        FolderStatus::Missing
                    }
                } else {
                    FolderStatus::Unmounted
                }
            }
        }
    }

    /// Arquivos recebidos por "Abrir com o Zek Music" (ou soltos na janela).
    fn open_files(&mut self, files: Vec<PathBuf>) {
        if files.is_empty() {
            return;
        }
        let id = self
            .cfg
            .playlists
            .iter()
            .find(|p| p.session)
            .map(|p| p.id)
            .unwrap_or_else(|| {
                let id = self.next_id();
                self.cfg.playlists.push(Playlist {
                    id,
                    name: "Abertos agora".into(),
                    source: PlaylistSource::Custom(vec![]),
                    session: true,
                });
                id
            });
        let mut novos = vec![];
        if let Some(pl) = self.cfg.playlists.iter_mut().find(|p| p.id == id) {
            if let PlaylistSource::Custom(v) = &mut pl.source {
                for f in files {
                    if !v.contains(&f) {
                        v.push(f.clone());
                        novos.push(f);
                    }
                }
            }
        }
        self.selected = Some(id);
        let alvo = self.tracks_of(id);
        let idx = novos.last().and_then(|u| alvo.iter().position(|x| x == u)).unwrap_or(0);
        let _ = self.cmd_tx.send(Cmd::PlayQueue { queue: alvo, index: idx, playlist: Some(id) });
        log_info(format!("tocando {} arquivo(s) recebido(s) do sistema", novos.len()));
    }

    // ── aviso de nova versão ──
    fn due_update_check(&self) -> bool {
        let hoje = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64 / 86400).unwrap_or(0);
        hoje > self.cfg.last_update_check
    }

    fn spawn_update_check(&mut self) {
        let tx = self.upd_tx.clone();
        let ctx = self.ctx.clone();
        self.cfg.last_update_check =
            SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64 / 86400).unwrap_or(0);
        self.save_config();
        std::thread::spawn(move || {
            match fetch_latest_release() {
                Some((tag, url)) => {
                    let _ = tx.send((tag, url));
                }
                None => log_warn("não foi possível consultar as versões no GitHub (sem internet?)"),
            }
            ctx.request_repaint();
        });
    }

    /// "v0.6.1" é mais novo que "v0.6.0"?
    fn nova_versao(&self, tag: &str) -> bool {
        versao_mais_nova(tag, env!("CARGO_PKG_VERSION"))
    }

    fn next_id(&mut self) -> u64 {
        self.cfg.next_id += 1;
        self.cfg.next_id
    }

    // ── biblioteca ──
    fn restart_watcher(&mut self) {
        self.watcher = None;
        if self.cfg.roots.is_empty() {
            return;
        }
        let tx = self.fs_tx.clone();
        let ctx = self.ctx.clone();
        let w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                if !matches!(ev.kind, notify::EventKind::Access(_)) {
                    let _ = tx.send(());
                    ctx.request_repaint();
                }
            }
        });
        if let Ok(mut w) = w {
            for r in &self.cfg.roots {
                let _ = w.watch(r, RecursiveMode::Recursive);
            }
            self.watcher = Some(w);
        }
    }

    fn start_scan(&mut self) {
        let (tx, rx) = mpsc::channel();
        let roots = self.cfg.roots.clone();
        // cópia do que já se sabe: evita reler as etiquetas de toda a biblioteca
        let known: HashMap<PathBuf, Meta> = self.meta.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let mut folders = scan_roots(&roots);
            let mut todas: Vec<PathBuf> = vec![];
            let mut vistos: HashSet<PathBuf> = HashSet::new();
            for v in folders.values() {
                for p in v {
                    if vistos.insert(p.clone()) {
                        todas.push(p.clone());
                    }
                }
            }
            let mut novas: Vec<(PathBuf, Meta)> = vec![];
            for p in &todas {
                if !known.contains_key(p) {
                    let (m, _) = read_info(p, None, false);
                    novas.push((p.clone(), m));
                }
            }
            let mut eff = known;
            for (p, m) in &novas {
                eff.insert(p.clone(), m.clone());
            }
            for v in folders.values_mut() {
                order_tracks(v, &eff);
            }
            let n = folders.len();
            let _ = tx.send(ScanMsg::Folders(folders));
            ctx.request_repaint();
            for chunk in novas.chunks(40) {
                if tx.send(ScanMsg::Metas(chunk.to_vec())).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
            let _ = tx.send(ScanMsg::Done);
            log_info(format!(
                "biblioteca atualizada: {} pastas, {} músicas novas, {:.1}s",
                n, novas.len(), t0.elapsed().as_secs_f32()
            ));
            ctx.request_repaint();
        });
        self.scan_rx = Some(rx);
    }

    fn apply_scan(&mut self, map: HashMap<PathBuf, Vec<PathBuf>>) {
        let roots = self.cfg.roots.clone();
        // pastas montadas? (pendrive/rede podem não estar montados ao abrir)
        self.refresh_root_ok();
        for (r, ok) in &self.root_ok {
            if !ok {
                log_warn(format!("pasta da biblioteca indisponível: {}", r.display()));
            }
        }
        // Some com a playlist só quando a pasta raiz está acessível e a pasta
        // realmente sumiu. Se o disco não está montado, GUARDA a playlist
        // (nomes e ordem) e avisa "pasta indisponível".
        let ok = self.root_ok.clone();
        self.cfg.playlists.retain(|p| match &p.source {
            PlaylistSource::Folder(path) => {
                let Some(raiz) = roots.iter().find(|r| path.starts_with(r)) else {
                    return false; // não pertence a nenhuma pasta da biblioteca
                };
                let montada = ok.get(raiz).copied().unwrap_or(true);
                // disco desmontado: guarda. disco montado e pasta sumiu: some.
                !montada || path.exists()
            }
            _ => true,
        });
        let mut keys: Vec<PathBuf> = map.keys().cloned().collect();
        keys.sort();
        for k in keys {
            let exists = self.cfg.playlists.iter().any(|p| matches!(&p.source, PlaylistSource::Folder(x) if *x == k));
            if !exists {
                let id = self.next_id();
                self.cfg.playlists.push(Playlist { id, name: folder_name(&k), source: PlaylistSource::Folder(k), session: false });
            }
        }
        self.tracks = map;
        if self.selected.map_or(true, |s| !self.cfg.playlists.iter().any(|p| p.id == s)) {
            self.selected = self.cfg.playlists.first().map(|p| p.id);
        }
        self.save_config();
    }

    fn tracks_of(&self, id: u64) -> Vec<PathBuf> {
        match self.cfg.playlists.iter().find(|p| p.id == id).map(|p| &p.source) {
            Some(PlaylistSource::Folder(path)) => self.tracks.get(path).cloned().unwrap_or_default(),
            Some(PlaylistSource::Custom(v)) => v.clone(),
            None => vec![],
        }
    }

    // ── reprodução ──
    fn eff_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.cfg.volume }
    }

    fn play_playlist(&mut self, id: u64, idx: usize) {
        let tr = self.tracks_of(id);
        if tr.is_empty() {
            return;
        }
        let _ = self.cmd_tx.send(Cmd::PlayQueue { queue: tr, index: idx, playlist: Some(id) });
    }

    // ── tick por frame ──
    fn tick(&mut self, ctx: &egui::Context) {
        while self.fs_rx.try_recv().is_ok() {
            self.dirty_since = Some(Instant::now());
        }
        if let Some(t) = self.dirty_since {
            if t.elapsed() >= Duration::from_millis(700) {
                self.dirty_since = None;
                self.start_scan();
            } else {
                ctx.request_repaint_after(Duration::from_millis(300));
            }
        }
        // Mensagens do scan em segundo plano
        let mut msgs: Vec<ScanMsg> = vec![];
        let mut gone = false;
        if let Some(rx) = &self.scan_rx {
            loop {
                match rx.try_recv() {
                    Ok(m) => msgs.push(m),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(_) => {
                        gone = true;
                        break;
                    }
                }
            }
        }
        for m in msgs {
            match m {
                ScanMsg::Folders(f) => self.apply_scan(f),
                ScanMsg::Metas(v) => {
                    for (p, mt) in v {
                        self.meta.insert(p, mt);
                    }
                }
                ScanMsg::Done => {
                    self.scan_rx = None;
                    // com todas as etiquetas lidas, refaz a ordem das faixas
                    for v in self.tracks.values_mut() {
                        order_tracks(v, &self.meta);
                    }
                }
            }
        }
        if gone {
            self.scan_rx = None;
        }
        self.try_restore();
        // Estado do motor de áudio (que roda em outra thread)
        let mut new_cover: Option<Option<Vec<u8>>> = None;
        {
            let g = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            self.snap = Snap {
                now: g.now.clone(),
                active: g.active,
                paused: g.paused,
                pos: g.pos,
                cur_path: g.cur_path.clone(),
                queue_pl: g.queue_pl,
                has_queue: g.has_queue,
                status: g.status.clone(),
            };
            if g.cover_gen != self.cover_gen {
                self.cover_gen = g.cover_gen;
                new_cover = Some(g.cover.clone());
            }
        }
        // A capa é decodificada fora da tela: capa grande não trava mais a janela.
        if let Some(c) = new_cover {
            match c {
                Some(bytes) => {
                    let tx = self.cover_tx.clone();
                    let gen = self.cover_gen;
                    std::thread::spawn(move || {
                        if let Some((img, color)) = decode_cover(&bytes) {
                            let _ = tx.send(CoverMsg { gen, img, color });
                        }
                    });
                }
                None => {
                    self.cover = None;
                    self.cover_color = None;
                }
            }
        }
        while let Ok(m) = self.cover_rx.try_recv() {
            if m.gen == self.cover_gen {
                self.cover_color = Some(m.color);
                self.cover = Some(self.ctx.load_texture("cover", m.img, egui::TextureOptions::LINEAR));
            }
        }
        // Resposta da consulta de nova versão
        while let Ok((tag, url)) = self.upd_rx.try_recv() {
            if self.nova_versao(&tag) {
                log_info(format!("versão nova no GitHub: {tag}"));
                self.update_info = Some((tag, url));
            } else {
                log_info(format!("nenhuma versão mais nova que {}", env!("CARGO_PKG_VERSION")));
            }
        }
        // Guarda de vez em quando o ponto onde parou (música, volume, posição)
        if self.last_session_save.elapsed() >= Duration::from_secs(15) {
            self.last_session_save = Instant::now();
            self.save_config();
        }
        if self.expanded && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.expanded = false;
        }
        if self.snap.now.is_none() {
            self.expanded = false;
        }
        if !self.seeking {
            self.seek_val = self.snap.pos;
        }
        if self.snap.active && !self.snap.paused {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        // Espaço = play/pause
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.handle(Act::Toggle);
        }
        // Arrastar arquivos para uma playlist personalizada
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            let mut files = vec![];
            for p in dropped {
                if p.is_dir() {
                    collect_audio(&p, &mut files, 0);
                } else if is_audio(&p) {
                    files.push(p);
                }
            }
            self.add_files_to_selected(files);
        }
    }

    /// Retoma a última música: só depois que a playlist tem as faixas carregadas.
    fn try_restore(&mut self) {
        let Some(s) = self.restore.clone() else { return };
        let Some(plid) = s.playlist else {
            self.restore = None;
            return;
        };
        let Some(pl) = self.cfg.playlists.iter().find(|p| p.id == plid) else {
            self.restore = None;
            return;
        };
        let estado = match &pl.source {
            PlaylistSource::Folder(p) => self.estado_pasta(p),
            PlaylistSource::Custom(_) => FolderStatus::Ok,
        };
        let tr = self.tracks_of(plid);
        if tr.is_empty() {
            // pasta desmontada ou apagada: não insiste em restaurar
            if estado != FolderStatus::Ok || self.scan_rx.is_none() {
                self.restore = None;
            }
            return;
        }
        if s.track.is_none() {
            // nada estava tocando ao fechar: só lembra a playlist escolhida
            self.selected = Some(plid);
            self.restore = None;
            return;
        }
        let idx = s.track.as_ref().and_then(|t| tr.iter().position(|x| x == t)).unwrap_or(0);
        let _ = self.cmd_tx.send(Cmd::Restore {
            queue: tr,
            index: idx,
            pos: s.pos.max(0.0),
            play: s.was_playing,
            playlist: Some(plid),
        });
        self.selected = Some(plid);
        self.restore = None;
        log_info("retomando de onde o programa parou");
    }

    fn add_files_to_selected(&mut self, files: Vec<PathBuf>) {
        let Some(sel) = self.selected else { return };
        let mut changed = false;
        if let Some(pl) = self.cfg.playlists.iter_mut().find(|p| p.id == sel) {
            if let PlaylistSource::Custom(v) = &mut pl.source {
                for f in files {
                    if !v.contains(&f) {
                        v.push(f);
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.save_config();
        }
    }

    // ── ações ──
    fn handle(&mut self, act: Act) {
        match act {
            Act::Select(id) => {
                self.selected = Some(id);
                self.search.clear();
            }
            Act::StartRename(id) => {
                if let Some(p) = self.cfg.playlists.iter().find(|p| p.id == id) {
                    self.renaming = Some((id, p.name.clone()));
                    self.rename_focus = true;
                }
            }
            Act::CommitRename => {
                if let Some((id, buf)) = self.renaming.take() {
                    let n = buf.trim().to_string();
                    if !n.is_empty() {
                        if let Some(p) = self.cfg.playlists.iter_mut().find(|p| p.id == id) {
                            p.name = n;
                        }
                        self.save_config();
                    }
                }
            }
            Act::CancelRename => self.renaming = None,
            Act::MoveUp(i) => {
                if i > 0 && i < self.cfg.playlists.len() {
                    self.cfg.playlists.swap(i, i - 1);
                    self.save_config();
                }
            }
            Act::MoveDown(i) => {
                if i + 1 < self.cfg.playlists.len() {
                    self.cfg.playlists.swap(i, i + 1);
                    self.save_config();
                }
            }
            Act::Delete(id) => {
                self.cfg.playlists.retain(|p| p.id != id);
                if self.selected == Some(id) {
                    self.selected = self.cfg.playlists.first().map(|p| p.id);
                }
                self.save_config();
            }
            Act::ResetName(id) => {
                if let Some(p) = self.cfg.playlists.iter_mut().find(|p| p.id == id) {
                    if let PlaylistSource::Folder(path) = &p.source {
                        p.name = folder_name(path);
                    }
                }
                self.save_config();
            }
            Act::NewPlaylist => {
                let id = self.next_id();
                self.cfg.playlists.push(Playlist { id, name: "Nova playlist".into(), source: PlaylistSource::Custom(vec![]), session: false });
                self.selected = Some(id);
                self.search.clear();
                self.save_config();
                self.handle(Act::StartRename(id));
            }
            Act::NewWith(path) => {
                let id = self.next_id();
                self.cfg.playlists.push(Playlist { id, name: "Nova playlist".into(), source: PlaylistSource::Custom(vec![path]), session: false });
                self.save_config();
            }
            Act::AddFolders => {
                if let Some(dirs) = rfd::FileDialog::new().set_title("Escolha a pasta das músicas").pick_folders() {
                    for d in dirs {
                        if !self.cfg.roots.contains(&d) {
                            self.cfg.roots.push(d);
                        }
                    }
                    self.save_config();
                    self.restart_watcher();
                    self.start_scan();
                }
            }
            Act::RemoveRoot(i) => {
                if i < self.cfg.roots.len() {
                    self.cfg.roots.remove(i);
                    self.save_config();
                    self.restart_watcher();
                    self.start_scan();
                }
            }
            Act::Refresh => self.start_scan(),
            Act::PlayAt(i) => {
                if let Some(sel) = self.selected {
                    self.play_playlist(sel, i);
                }
            }
            Act::PlayAll => {
                if let Some(sel) = self.selected {
                    let n = self.tracks_of(sel).len();
                    let start = if self.shuffle && n > 1 { rand::thread_rng().gen_range(0..n) } else { 0 };
                    self.play_playlist(sel, start);
                }
            }
            Act::AddTo(id, path) => {
                if let Some(pl) = self.cfg.playlists.iter_mut().find(|p| p.id == id) {
                    if let PlaylistSource::Custom(v) = &mut pl.source {
                        if !v.contains(&path) {
                            v.push(path);
                        }
                    }
                }
                self.save_config();
            }
            Act::RemoveTrack(i) => {
                if let Some(sel) = self.selected {
                    if let Some(pl) = self.cfg.playlists.iter_mut().find(|p| p.id == sel) {
                        if let PlaylistSource::Custom(v) = &mut pl.source {
                            if i < v.len() {
                                v.remove(i);
                            }
                        }
                    }
                    self.save_config();
                }
            }
            Act::AddFiles => {
                if let Some(files) = rfd::FileDialog::new().set_title("Adicionar músicas").add_filter("Áudio", AUDIO_EXT).pick_files() {
                    self.add_files_to_selected(files);
                }
            }
            Act::MovePlaylistTo(origem, dest) => {
                if let Some(de) = self.cfg.playlists.iter().position(|p| p.id == origem) {
                    let item = self.cfg.playlists.remove(de);
                    let mut d = dest.min(self.cfg.playlists.len());
                    if de < d {
                        d -= 1;
                    }
                    self.cfg.playlists.insert(d, item);
                    self.save_config();
                }
            }
            Act::SetAccent(i) => {
                self.cfg.accent_custom = None;
                self.cfg.accent_idx = i;
                let a = Accent::from_cfg(&self.cfg);
                self.accent = a;
                setup_style(&self.ctx, a);
                self.save_config();
                self.ctx.request_repaint();
            }
            Act::SetAccentCustom(cor) => {
                self.cfg.accent_custom = cor;
                let a = Accent::from_cfg(&self.cfg);
                self.accent = a;
                setup_style(&self.ctx, a);
                self.save_config();
                self.ctx.request_repaint();
            }
            Act::SetCheckUpdates(v) => {
                self.cfg.check_updates = v;
                self.save_config();
            }
            Act::CheckUpdates => {
                self.upd_dismissed = false;
                self.spawn_update_check();
            }
            Act::OpenDataDir => {
                let d = data_dir();
                let _ = fs::create_dir_all(&d);
                let r = if cfg!(target_os = "windows") {
                    std::process::Command::new("explorer").arg(&d).spawn()
                } else {
                    std::process::Command::new("xdg-open").arg(&d).spawn()
                };
                if r.is_err() {
                    log_warn(format!("não foi possível abrir {}", d.display()));
                }
            }
            Act::Toggle => {
                if self.snap.active || self.snap.has_queue {
                    let _ = self.cmd_tx.send(Cmd::Toggle);
                } else if let Some(sel) = self.selected {
                    self.play_playlist(sel, 0);
                }
            }
            Act::Next => {
                let _ = self.cmd_tx.send(Cmd::Next);
            }
            Act::Prev => {
                let _ = self.cmd_tx.send(Cmd::Prev);
            }
            Act::Seek(t) => {
                let _ = self.cmd_tx.send(Cmd::Seek(t));
            }
            Act::VolChanged => {
                let v = self.eff_volume();
                let _ = self.cmd_tx.send(Cmd::Volume(v));
            }
        }
    }

    // ───────────────────────── UI: barra do player ─────────────────────────
    fn ui_player(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let bar_resp = egui::TopBottomPanel::bottom("player")
            .exact_height(98.0)
            .frame(Frame::none().fill(PANEL).inner_margin(Margin::symmetric(20.0, 12.0)))
            .show(ctx, |ui| {
                let total = ui.available_width();
                let left_w = 300.0;
                let right_w = 190.0;
                let mid_w = (total - left_w - right_w - 16.0).max(240.0);
                ui.horizontal(|ui| {
                    // ── esquerda: capa + faixa atual
                    let left_origin = ui.cursor().min;
                    ui.allocate_ui_with_layout(Vec2::new(left_w, 72.0), Layout::left_to_right(Align::Center), |ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                        if let Some(tex) = &self.cover {
                            ui.put(
                                r,
                                egui::Image::from_texture(egui::load::SizedTexture::from_handle(tex))
                                    .fit_to_exact_size(Vec2::splat(64.0))
                                    .rounding(10.0),
                            );
                        } else if let Some(n) = &self.snap.now {
                            let (c1, c2) = tile_colors(&n.title);
                            rounded_gradient(ui.painter(), r, 10.0, c1, c2);
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, initial(&n.title), FontId::proportional(28.0), Color32::from_white_alpha(210));
                        } else {
                            rounded_gradient(ui.painter(), r, 10.0, BTN, PANEL);
                        }
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.set_max_width(left_w - 84.0);
                            if let Some(n) = &self.snap.now {
                                ui.add(egui::Label::new(RichText::new(&n.title).size(15.5).strong().color(Color32::WHITE)).wrap_mode(egui::TextWrapMode::Truncate));
                                let a = if n.artist.is_empty() { "Artista desconhecido" } else { n.artist.as_str() };
                                ui.add(egui::Label::new(RichText::new(a).size(12.5).color(MUTED)).wrap_mode(egui::TextWrapMode::Truncate));
                            } else {
                                ui.label(RichText::new("Nada tocando").size(15.0).color(MUTED));
                                ui.label(RichText::new("Escolha uma música").size(12.5).color(MUTED));
                            }
                        });
                    });
                    {
                        let hit_rect = Rect::from_min_size(left_origin, Vec2::new(left_w, 72.0));
                        let hit = ui.interact(hit_rect, ui.id().with("np_toggle"), Sense::click());
                        let can = self.snap.now.is_some();
                        if can {
                            let col = if hit.hovered() { Color32::WHITE } else { MUTED };
                            chevron(ui.painter(), Pos2::new(hit_rect.right() - 12.0, hit_rect.center().y), !self.expanded, col);
                            if hit.clicked() {
                                self.expanded = !self.expanded;
                            }
                            hit.on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(if self.expanded { "Recolher" } else { "Expandir capa" });
                        }
                    }
                    // ── centro: controles + linha do tempo
                    ui.allocate_ui_with_layout(Vec2::new(mid_w, 72.0), Layout::top_down(Align::LEFT), |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        ui.allocate_ui_with_layout(Vec2::new(mid_w, 46.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 14.0;
                            ui.add_space(((mid_w - 230.0) / 2.0).max(0.0));
                            if icon_button(ui, Icon::Shuffle, 30.0, false, self.shuffle, self.accent).on_hover_text("Aleatório").clicked() {
                                self.shuffle = !self.shuffle;
                                let _ = self.cmd_tx.send(Cmd::Shuffle(self.shuffle));
                            }
                            if icon_button(ui, Icon::Prev, 34.0, false, false, self.accent).clicked() {
                                acts.push(Act::Prev);
                            }
                            let playing = self.snap.active && !self.snap.paused;
                            let ic = if playing { Icon::Pause } else { Icon::Play };
                            if icon_button(ui, ic, 46.0, true, false, self.accent).clicked() {
                                acts.push(Act::Toggle);
                            }
                            if icon_button(ui, Icon::Next, 34.0, false, false, self.accent).clicked() {
                                acts.push(Act::Next);
                            }
                            let (ri, tip) = match self.repeat {
                                Repeat::Off => (Icon::Repeat, "Repetir: desligado"),
                                Repeat::All => (Icon::Repeat, "Repetir: tudo"),
                                Repeat::One => (Icon::RepeatOne, "Repetir: uma música"),
                            };
                            if icon_button(ui, ri, 30.0, false, self.repeat != Repeat::Off, self.accent).on_hover_text(tip).clicked() {
                                self.repeat = match self.repeat {
                                    Repeat::Off => Repeat::All,
                                    Repeat::All => Repeat::One,
                                    Repeat::One => Repeat::Off,
                                };
                                let _ = self.cmd_tx.send(Cmd::Repeat(self.repeat));
                            }
                        });
                        ui.allocate_ui_with_layout(Vec2::new(mid_w, 20.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            let dur = self.snap.now.as_ref().map_or(0.0, |n| n.duration);
                            let max = dur.max(self.seek_val + 0.5).max(1.0);
                            ui.add_sized([36.0, 16.0], egui::Label::new(RichText::new(fmt_time(self.seek_val)).size(12.0).color(MUTED)));
                            let mut v = self.seek_val;
                            let out = bar(ui, &mut v, max, (mid_w - 36.0 * 2.0 - 16.0).max(60.0), self.snap.active, self.accent);
                            self.seeking = out.dragging;
                            if out.changed {
                                self.seek_val = v;
                            }
                            if out.released {
                                acts.push(Act::Seek(v));
                            }
                            ui.add_sized([36.0, 16.0], egui::Label::new(RichText::new(fmt_time(dur)).size(12.0).color(MUTED)));
                        });
                    });
                    // ── direita: volume
                    ui.allocate_ui_with_layout(Vec2::new(right_w, 72.0), Layout::right_to_left(Align::Center), |ui| {
                        let mut changed = false;
                        let shown = if self.muted { 0.0 } else { self.cfg.volume };
                        let mut v = shown;
                        let out = bar(ui, &mut v, 1.0, 110.0, true, self.accent);
                        if out.changed {
                            self.cfg.volume = v;
                            self.muted = false;
                            changed = true;
                        }
                        let ic = if self.muted || self.cfg.volume <= 0.001 { Icon::Muted } else { Icon::Volume };
                        if icon_button(ui, ic, 32.0, false, false, self.accent).on_hover_text("Silenciar").clicked() {
                            self.muted = !self.muted;
                            changed = true;
                        }
                        if changed {
                            acts.push(Act::VolChanged);
                        }
                    });
                });
            });
        self.bar_top = bar_resp.response.rect.top();
    }

    // ───────────────────────── UI: barra lateral ─────────────────────────
    fn ui_sidebar(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        egui::SidePanel::left("side")
            .exact_width(262.0)
            .resizable(false)
            .frame(Frame::none().fill(PANEL).inner_margin(Margin::symmetric(12.0, 16.0)))
            .show(ctx, |ui| {
                let time = ui.input(|i| i.time);
                let acc = self.accent;
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
                    rounded_gradient(ui.painter(), r, 11.0, acc.base, acc.hi);
                    let c = r.center();
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![Pos2::new(c.x - 4.5, c.y - 7.0), Pos2::new(c.x - 4.5, c.y + 7.0), Pos2::new(c.x + 8.0, c.y)],
                        Color32::WHITE,
                        Stroke::NONE,
                    ));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Zek Music").size(22.0).strong().color(Color32::WHITE));
                });
                ui.add_space(10.0);
                // ── aviso de nova versão (some em "Depois") ──
                if let Some((tag, url)) = self.update_info.clone() {
                    if !self.upd_dismissed {
                        egui::Frame::none()
                            .fill(BTN)
                            .stroke(stroke(1.0, acc.hi))
                            .rounding(Rounding::same(12.0))
                            .inner_margin(Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(
                                    RichText::new(format!("Nova versão {tag} disponível"))
                                        .size(13.0)
                                        .color(Color32::WHITE),
                                );
                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(RichText::new("Baixar").size(12.0))
                                                .fill(acc.base)
                                                .min_size(Vec2::new(78.0, 24.0)),
                                        )
                                        .clicked()
                                    {
                                        open_url(&url);
                                    }
                                    if ui
                                        .add(
                                            egui::Button::new(RichText::new("Depois").size(12.0))
                                                .min_size(Vec2::new(58.0, 24.0)),
                                        )
                                        .clicked()
                                    {
                                        self.upd_dismissed = true;
                                    }
                                });
                            });
                        ui.add_space(8.0);
                    }
                }
                ui.label(RichText::new("   SUAS PLAYLISTS").size(11.0).strong().color(MUTED));
                ui.add_space(6.0);
                let h = (ui.available_height() - 142.0).max(80.0);
                egui::ScrollArea::vertical().max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    for (i, p) in self.cfg.playlists.iter().enumerate() {
                        let selected = self.selected == Some(p.id);
                        let renaming_this = matches!(&self.renaming, Some((rid, _)) if *rid == p.id);
                        if renaming_this {
                            let focus = self.rename_focus;
                            self.rename_focus = false;
                            if let Some((_, buf)) = self.renaming.as_mut() {
                                let r = ui.add(egui::TextEdit::singleline(buf).desired_width(f32::INFINITY));
                                if focus {
                                    r.request_focus();
                                }
                                if r.lost_focus() {
                                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                        acts.push(Act::CancelRename);
                                    } else {
                                        acts.push(Act::CommitRename);
                                    }
                                }
                            }
                            continue;
                        }
                        let (rect, resp) =
                            ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.0), Sense::click_and_drag());
                        let (count, custom, estado) = match &p.source {
                            PlaylistSource::Folder(x) => (self.tracks.get(x).map_or(0, |v| v.len()), false, self.estado_pasta(x)),
                            PlaylistSource::Custom(v) => (v.len(), true, FolderStatus::Ok),
                        };
                        let indisponivel = estado != FolderStatus::Ok;
                        // ── arrastar para reordenar ──
                        if resp.drag_started() && !p.session {
                            self.drag_pl = Some(p.id);
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                        let arrastando = self.drag_pl == Some(p.id);
                        if arrastando {
                            ui.painter().rect_filled(rect, 12.0, Color32::from_white_alpha(22));
                        } else if let Some(origem) = self.drag_pl {
                            if origem != p.id && !p.session {
                                if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                                    if rect.contains(pos) {
                                        let antes = pos.y < rect.center().y;
                                        let y = if antes { rect.top() } else { rect.bottom() };
                                        ui.painter().rect_filled(rect, 12.0, Color32::from_white_alpha(12));
                                        ui.painter().hline(rect.x_range(), y, stroke(2.0, acc.hi));
                                        self.drag_target = Some(if antes { i } else { i + 1 });
                                    }
                                }
                            }
                        }
                        let is_playing = self.snap.queue_pl == Some(p.id) && self.snap.active && !self.snap.paused;
                        {
                            let painter = ui.painter();
                            if selected {
                                painter.rect_filled(rect, 12.0, SEL_BG);
                                painter.rect_filled(Rect::from_min_size(rect.min + Vec2::new(0.0, 12.0), Vec2::new(3.0, rect.height() - 24.0)), 2.0, ACCENT);
                            } else if resp.hovered() {
                                painter.rect_filled(rect, 12.0, ROW_HOVER);
                            }
                            let tile = Rect::from_min_size(Pos2::new(rect.left() + 12.0, rect.center().y - 18.0), Vec2::splat(36.0));
                            let (c1, c2) = tile_colors(&p.id.to_string());
                            rounded_gradient(painter, tile, 9.0, c1, c2);
                            painter.text(tile.center(), Align2::CENTER_CENTER, initial(&p.name), FontId::proportional(16.0), Color32::from_white_alpha(225));
                            let right = if selected { rect.right() - 58.0 } else { rect.right() - 34.0 };
                            let clip = Rect::from_min_max(rect.min, Pos2::new(right, rect.max.y));
                            let pc = painter.with_clip_rect(clip);
                            let sub = if indisponivel {
                                "pasta indisponível".to_string()
                            } else {
                                format!("{} músicas{}", count, if custom { " • personalizada" } else { "" })
                            };
                            let cor_nome = if indisponivel { Color32::from_white_alpha(110) } else { TEXT };
                            pc.text(
                                Pos2::new(rect.left() + 58.0, rect.center().y - 9.0),
                                Align2::LEFT_CENTER,
                                &p.name,
                                FontId::proportional(14.5),
                                if selected { Color32::WHITE } else { cor_nome },
                            );
                            pc.text(Pos2::new(rect.left() + 58.0, rect.center().y + 10.0), Align2::LEFT_CENTER, sub, FontId::proportional(11.5), if indisponivel { WARN } else { MUTED });
                            if !selected && is_playing {
                                equalizer(painter, Pos2::new(rect.right() - 18.0, rect.center().y), time, true, self.accent.hi);
                            }
                        }
                        if selected {
                            let up = Rect::from_center_size(Pos2::new(rect.right() - 38.0, rect.center().y), Vec2::splat(22.0));
                            let dn = Rect::from_center_size(Pos2::new(rect.right() - 14.0, rect.center().y), Vec2::splat(22.0));
                            if arrow_btn(ui, up, true, ui.id().with(("up", p.id))).clicked() {
                                acts.push(Act::MoveUp(i));
                            }
                            if arrow_btn(ui, dn, false, ui.id().with(("dn", p.id))).clicked() {
                                acts.push(Act::MoveDown(i));
                            }
                        }
                        if resp.clicked() {
                            acts.push(Act::Select(p.id));
                        }
                        if resp.double_clicked() {
                            acts.push(Act::StartRename(p.id));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Renomear").clicked() {
                                acts.push(Act::StartRename(p.id));
                                ui.close_menu();
                            }
                            if ui.button("Mover para cima").clicked() {
                                acts.push(Act::MoveUp(i));
                                ui.close_menu();
                            }
                            if ui.button("Mover para baixo").clicked() {
                                acts.push(Act::MoveDown(i));
                                ui.close_menu();
                            }
                            match &p.source {
                                PlaylistSource::Custom(_) => {
                                    if !p.session && ui.button("Excluir playlist").clicked() {
                                        acts.push(Act::Delete(p.id));
                                        ui.close_menu();
                                    }
                                }
                                PlaylistSource::Folder(_) => {
                                    if ui.button("Restaurar nome da pasta").clicked() {
                                        acts.push(Act::ResetName(p.id));
                                        ui.close_menu();
                                    }
                                }
                            }
                        });
                    }
                });
                // soltou a playlist arrastada? aplica a nova posição
                if let Some(origem) = self.drag_pl {
                    if ui.input(|i| !i.pointer.any_down()) {
                        self.drag_pl = None;
                        if let Some(dest) = self.drag_target.take() {
                            acts.push(Act::MovePlaylistTo(origem, dest));
                        }
                    }
                }
                ui.add_space(10.0);
                let w = ui.available_width();
                if ui.add_sized([w, 36.0], egui::Button::new(RichText::new("+  Nova playlist").color(Color32::WHITE).strong()).fill(acc.base)).clicked() {
                    acts.push(Act::NewPlaylist);
                }
                ui.horizontal(|ui| {
                    let bw = (ui.available_width() - 6.0) / 2.0;
                    if ui.add_sized([bw, 36.0], egui::Button::new("Pastas")).on_hover_text("Pastas da biblioteca").clicked() {
                        self.show_folders = true;
                    }
                    if ui.add_sized([bw, 36.0], egui::Button::new("Ajustes")).clicked() {
                        self.show_settings = true;
                    }
                });
            });
    }

    // ───────────────────────── UI: área central ─────────────────────────
    fn ui_central(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        egui::CentralPanel::default().frame(Frame::none().fill(BG)).show(ctx, |ui| {
            let full = ui.max_rect();
            let gh = full.height().min(380.0);
            let acc = self.accent;
            gradient_rect_v(ui.painter(), Rect::from_min_size(full.min, Vec2::new(full.width(), gh)), scale_color(acc.base, 0.42), BG);

            let has_custom = self.cfg.playlists.iter().any(|p| matches!(p.source, PlaylistSource::Custom(_)));
            if self.cfg.roots.is_empty() && !has_custom {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.26);
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(84.0), Sense::hover());
                    rounded_gradient(ui.painter(), r, 24.0, acc.base, acc.hi);
                    let c = r.center();
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![Pos2::new(c.x - 10.0, c.y - 16.0), Pos2::new(c.x - 10.0, c.y + 16.0), Pos2::new(c.x + 18.0, c.y)],
                        Color32::WHITE,
                        Stroke::NONE,
                    ));
                    ui.add_space(14.0);
                    ui.label(RichText::new("Zek Music").size(40.0).strong().color(Color32::WHITE));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Escolha a pasta que contém as suas pastas de músicas.\nCada subpasta vira uma playlist automaticamente.").size(16.0).color(MUTED));
                    ui.add_space(20.0);
                    let b = egui::Button::new(RichText::new("Escolher pasta de músicas").size(16.0).color(Color32::WHITE).strong())
                        .fill(acc.base)
                        .min_size(Vec2::new(280.0, 46.0));
                    if ui.add(b).clicked() {
                        acts.push(Act::AddFolders);
                    }
                });
                return;
            }
            let Some(sel) = self.selected else {
                ui.label(RichText::new("Selecione uma playlist ao lado.").color(MUTED));
                return;
            };
            let Some(pl) = self.cfg.playlists.iter().find(|p| p.id == sel) else { return };
            let is_custom = matches!(pl.source, PlaylistSource::Custom(_));
            let estado = match &pl.source {
                PlaylistSource::Folder(p) => self.estado_pasta(p),
                PlaylistSource::Custom(_) => FolderStatus::Ok,
            };
            let name = pl.name.clone();
            let tracks: Vec<PathBuf> = match &pl.source {
                PlaylistSource::Folder(p) => self.tracks.get(p).cloned().unwrap_or_default(),
                PlaylistSource::Custom(v) => v.clone(),
            };
            let customs: Vec<(u64, String)> = self
                .cfg
                .playlists
                .iter()
                .filter(|p| matches!(p.source, PlaylistSource::Custom(_)) && p.id != sel)
                .map(|p| (p.id, p.name.clone()))
                .collect();
            let cur_path: Option<PathBuf> = if self.snap.queue_pl == Some(sel) && self.snap.active { self.snap.cur_path.clone() } else { None };
            let total_secs: f32 = tracks.iter().filter_map(|t| self.meta.get(t)).map(|m| m.duration).sum();
            let time_now = ui.input(|i| i.time);
            let playing_anim = self.snap.active && !self.snap.paused;

            Frame::none().inner_margin(Margin { left: 30.0, right: 30.0, top: 30.0, bottom: 0.0 }).show(ui, |ui| {
                // ── cabeçalho
                ui.horizontal(|ui| {
                    let (cr, _) = ui.allocate_exact_size(Vec2::splat(150.0), Sense::hover());
                    let (c1, c2) = tile_colors(&sel.to_string());
                    rounded_gradient(ui.painter(), cr, 18.0, c1, c2);
                    ui.painter().text(cr.center(), Align2::CENTER_CENTER, initial(&name), FontId::proportional(70.0), Color32::from_white_alpha(205));
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.add_space(4.0);
                        ui.label(RichText::new(if is_custom { "PLAYLIST PERSONALIZADA" } else { "PLAYLIST" }).size(11.5).strong().color(ACCENT_HI));
                        let r = ui.add(
                            egui::Label::new(RichText::new(&name).size(42.0).strong().color(Color32::WHITE))
                                .wrap_mode(egui::TextWrapMode::Truncate)
                                .sense(Sense::click()),
                        );
                        if r.double_clicked() {
                            acts.push(Act::StartRename(sel));
                        }
                        r.on_hover_text("Duplo clique para renomear");
                        let mut sub = format!("{} músicas", tracks.len());
                        if total_secs > 60.0 {
                            sub.push_str(&format!("  •  {}", fmt_total(total_secs)));
                        }
                        if self.scan_rx.is_some() {
                            sub.push_str("  •  atualizando...");
                        }
                        ui.label(RichText::new(sub).size(14.0).color(MUTED));
                        if !self.snap.status.is_empty() {
                            ui.label(RichText::new(&self.snap.status).size(13.0).color(WARN));
                        }
                        ui.add_space(8.0);
                        ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), 56.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 14.0;
                            let same = self.snap.queue_pl == Some(sel) && self.snap.active;
                            let this_playing = same && !self.snap.paused;
                            let ic = if this_playing { Icon::Pause } else { Icon::Play };
                            if icon_button(ui, ic, 54.0, true, false, self.accent).clicked() {
                                acts.push(if same { Act::Toggle } else { Act::PlayAll });
                            }
                            if icon_button(ui, Icon::Shuffle, 38.0, false, self.shuffle, self.accent).on_hover_text("Aleatório").clicked() {
                                self.shuffle = !self.shuffle;
                                let _ = self.cmd_tx.send(Cmd::Shuffle(self.shuffle));
                            }
                            if is_custom && ui.add(egui::Button::new("Adicionar músicas").min_size(Vec2::new(150.0, 34.0))).clicked() {
                                acts.push(Act::AddFiles);
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Buscar nesta playlist...").desired_width(230.0).margin(Vec2::new(10.0, 8.0)));
                            });
                        });
                    });
                });
                ui.add_space(22.0);

                if tracks.is_empty() {
                    ui.add_space(24.0);
                    let msg = if is_custom {
                        "Playlist vazia. Use \"Adicionar músicas\", arraste arquivos para cá\nou clique com o botão direito numa música de outra playlist."
                    } else if estado == FolderStatus::Unmounted || estado == FolderStatus::Missing {
                        "Pasta indisponível agora (o disco pode estar desmontado).\n\
                         Suas playlists e a ordem continuam guardadas — quando a pasta voltar, tudo reaparece."
                    } else {
                        "Nenhuma música encontrada nesta pasta."
                    };
                    ui.label(RichText::new(msg).size(15.0).color(MUTED));
                    return;
                }

                // ── cabeçalho da tabela
                let (hr, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
                {
                    let (tx, ax, dx) = columns(hr);
                    let p = ui.painter();
                    let f = FontId::proportional(11.5);
                    p.text(Pos2::new(hr.left() + 24.0, hr.center().y), Align2::CENTER_CENTER, "#", f.clone(), MUTED);
                    p.text(Pos2::new(tx, hr.center().y), Align2::LEFT_CENTER, "TÍTULO", f.clone(), MUTED);
                    p.text(Pos2::new(ax, hr.center().y), Align2::LEFT_CENTER, "ARTISTA", f.clone(), MUTED);
                    p.text(Pos2::new(dx, hr.center().y), Align2::RIGHT_CENTER, "DURAÇÃO", f, MUTED);
                    p.hline(hr.x_range(), hr.bottom(), stroke(1.0, Color32::from_rgb(30, 44, 84)));
                }
                ui.add_space(4.0);

                let q = self.search.trim().to_lowercase();
                let filtered: Vec<usize> = if q.is_empty() {
                    (0..tracks.len()).collect()
                } else {
                    (0..tracks.len())
                        .filter(|&i| {
                            let t = &tracks[i];
                            let mut hay = clean_title(&stem(t)).to_lowercase();
                            if let Some(m) = self.meta.get(t) {
                                hay.push(' ');
                                hay.push_str(&m.title.to_lowercase());
                                hay.push(' ');
                                hay.push_str(&m.artist.to_lowercase());
                            }
                            hay.contains(&q)
                        })
                        .collect()
                };

                let row_h = 46.0;
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, filtered.len(), |ui, range| {
                    for fi in range {
                        let ti = filtered[fi];
                        let path = &tracks[ti];
                        let playing = cur_path.as_ref() == Some(path);
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());
                        let m = self.meta.get(path);
                        let title = m.map(|m| m.title.clone()).unwrap_or_else(|| clean_title(&stem(path)));
                        let artist = m.map(|m| m.artist.clone()).unwrap_or_default();
                        let dur = m.map_or(0.0, |m| m.duration);
                        {
                            let painter = ui.painter();
                            if playing {
                                painter.rect_filled(rect, 10.0, SEL_BG);
                            } else if resp.hovered() {
                                painter.rect_filled(rect, 10.0, ROW_HOVER);
                            }
                            let cy = rect.center().y;
                            let (tx, ax, dx) = columns(rect);
                            if playing {
                                equalizer(painter, Pos2::new(rect.left() + 24.0, cy), time_now, playing_anim, self.accent.hi);
                            } else if resp.hovered() {
                                painter.add(egui::Shape::convex_polygon(
                                    vec![Pos2::new(rect.left() + 20.0, cy - 6.0), Pos2::new(rect.left() + 20.0, cy + 6.0), Pos2::new(rect.left() + 30.0, cy)],
                                    Color32::WHITE,
                                    Stroke::NONE,
                                ));
                            } else {
                                painter.text(Pos2::new(rect.left() + 24.0, cy), Align2::CENTER_CENTER, (ti + 1).to_string(), FontId::proportional(13.0), MUTED);
                            }
                            let tclip = Rect::from_min_max(rect.min, Pos2::new((ax - 14.0).max(tx), rect.max.y));
                            painter.with_clip_rect(tclip).text(Pos2::new(tx, cy), Align2::LEFT_CENTER, title, FontId::proportional(15.0), if playing { ACCENT_HI } else { TEXT });
                            if !artist.is_empty() {
                                let aclip = Rect::from_min_max(Pos2::new(ax, rect.min.y), Pos2::new((dx - 64.0).max(ax), rect.max.y));
                                painter.with_clip_rect(aclip).text(Pos2::new(ax, cy), Align2::LEFT_CENTER, artist, FontId::proportional(13.5), MUTED);
                            }
                            if dur > 0.0 {
                                painter.text(Pos2::new(dx, cy), Align2::RIGHT_CENTER, fmt_time(dur), FontId::proportional(13.5), MUTED);
                            }
                        }
                        if resp.clicked() {
                            acts.push(Act::PlayAt(ti));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Tocar").clicked() {
                                acts.push(Act::PlayAt(ti));
                                ui.close_menu();
                            }
                            ui.menu_button("Adicionar à playlist", |ui| {
                                for (id, n) in &customs {
                                    if ui.button(n).clicked() {
                                        acts.push(Act::AddTo(*id, path.clone()));
                                        ui.close_menu();
                                    }
                                }
                                if ui.button("+ Nova playlist").clicked() {
                                    acts.push(Act::NewWith(path.clone()));
                                    ui.close_menu();
                                }
                            });
                            if is_custom && ui.button("Remover desta playlist").clicked() {
                                acts.push(Act::RemoveTrack(ti));
                                ui.close_menu();
                            }
                        });
                    }
                });
            });
        });
    }

    // ───────────────────────── UI: tela "Tocando agora" ─────────────────────────
    fn ui_expanded(&mut self, ctx: &egui::Context) {
        let t = ctx.animate_bool_with_time(egui::Id::new("np_open"), self.expanded, 0.22);
        if t <= 0.001 {
            return;
        }
        let Some(now) = self.snap.now.clone() else { return };
        let e = 1.0 - (1.0 - t).powi(3);
        let screen = ctx.screen_rect();
        let bottom = if self.bar_top > screen.top() { self.bar_top } else { screen.bottom() - 98.0 };
        let full = Rect::from_min_max(screen.min, Pos2::new(screen.max.x, bottom));
        let rect = full.translate(Vec2::new(0.0, (1.0 - e) * full.height()));
        let (tile1, tile2) = tile_colors(&now.title);
        let has_cover = self.cover.is_some();
        let (top_a, top_b) = match self.cover_color {
            Some(c) => (c, lerp_color(c, ACCENT, 0.35)),
            None => (scale_color(tile1, 0.62), scale_color(tile2, 0.62)),
        };
        let mut close = false;
        egui::Area::new(egui::Id::new("np_area")).order(egui::Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
            ui.set_clip_rect(full);
            ui.allocate_exact_size(rect.size(), Sense::click());
            let p = ui.painter().clone();
            gradient_rect_4(&p, rect, top_a, top_b, BG, BG);

            let w = rect.width();
            let h = rect.height();
            let s = (h * 0.60).min(w * 0.42).min(h - 170.0).max(120.0);
            let block_h = s + 118.0;
            let top = rect.top() + ((h - block_h) / 2.0).max(36.0);
            let shift = if has_cover { 0.0 } else { s * 0.13 };
            let cx = rect.center().x;
            let cover = Rect::from_center_size(Pos2::new(cx - shift, top + s / 2.0), Vec2::splat(s));

            // sombra suave atrás da capa
            for i in 0..5 {
                let grow = i as f32 * 5.0;
                p.rect_filled(cover.expand(grow).translate(Vec2::new(0.0, 10.0)), 22.0 + grow, Color32::from_black_alpha(16));
            }
            // sem capa: um disco de vinil saindo por trás do quadrado
            if !has_cover {
                let vc = Pos2::new(cover.center().x + s * 0.30, cover.center().y);
                let r = s * 0.47;
                let dark = Color32::from_rgb(10, 12, 20);
                p.circle_filled(vc, r, dark);
                for k in [0.62_f32, 0.74, 0.86, 0.96] {
                    p.circle_stroke(vc, r * k, stroke(1.0, Color32::from_white_alpha(16)));
                }
                p.circle_filled(vc, r * 0.32, lerp_color(tile1, tile2, 0.5));
                p.circle_filled(vc, r * 0.05, dark);
            }
            if let Some(tex) = &self.cover {
                ui.put(
                    cover,
                    egui::Image::from_texture(egui::load::SizedTexture::from_handle(tex))
                        .fit_to_exact_size(Vec2::splat(s))
                        .rounding(18.0),
                );
            } else {
                rounded_gradient(&p, cover, 18.0, tile1, tile2);
                p.text(cover.center(), Align2::CENTER_CENTER, initial(&now.title), FontId::proportional(s * 0.42), Color32::from_white_alpha(215));
            }

            // textos
            let tw = (w * 0.80).min(900.0);
            let ty = cover.bottom() + 28.0;
            let tx = Pos2::new(cx, ty);
            let clip = Rect::from_min_size(Pos2::new(cx - tw / 2.0, ty - 2.0), Vec2::new(tw, 112.0));
            let pc = p.with_clip_rect(clip);
            pc.text(tx, Align2::CENTER_TOP, &now.title, FontId::proportional(28.0), Color32::WHITE);
            let artist = if now.artist.is_empty() { "Artista desconhecido" } else { now.artist.as_str() };
            pc.text(Pos2::new(cx, ty + 42.0), Align2::CENTER_TOP, artist, FontId::proportional(16.0), Color32::from_rgb(196, 210, 238));
            if !now.album.is_empty() {
                pc.text(Pos2::new(cx, ty + 68.0), Align2::CENTER_TOP, &now.album, FontId::proportional(13.5), MUTED);
            }

            // botão de recolher (canto superior direito)
            let br = Rect::from_center_size(Pos2::new(rect.right() - 38.0, rect.top() + 36.0), Vec2::splat(42.0));
            let r = ui.interact(br, ui.id().with("np_close"), Sense::click());
            if r.hovered() {
                p.circle_filled(br.center(), 21.0, Color32::from_white_alpha(26));
            }
            chevron(&p, br.center(), false, Color32::WHITE);
            if r.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Recolher").clicked() {
                close = true;
            }
        });
        if close {
            self.expanded = false;
        }
    }

    // ───────────────────────── UI: janela de pastas ─────────────────────────
    fn ui_folders(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        if !self.show_folders {
            return;
        }
        let mut open = true;
        let acc = self.accent;
        egui::Window::new(RichText::new("Pastas da biblioteca").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(480.0);
                ui.label(RichText::new("Escolha a pasta que CONTÉM as suas pastas de músicas (por exemplo, \"musicas\"). Cada subpasta vira uma playlist.").color(MUTED));
                ui.add_space(10.0);
                if self.cfg.roots.is_empty() {
                    ui.label(RichText::new("Nenhuma pasta adicionada ainda.").color(MUTED));
                }
                for (i, r) in self.cfg.roots.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.button("Remover").clicked() {
                            acts.push(Act::RemoveRoot(i));
                        }
                        ui.add(egui::Label::new(RichText::new(r.to_string_lossy()).color(TEXT)).wrap_mode(egui::TextWrapMode::Truncate));
                    });
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(RichText::new("Adicionar pasta").color(Color32::WHITE).strong()).fill(acc.base)).clicked() {
                        acts.push(Act::AddFolders);
                    }
                    if ui.button("Atualizar biblioteca").clicked() {
                        acts.push(Act::Refresh);
                    }
                });
            });
        self.show_folders = open;
    }

    // ───────────────────────── UI: ajustes ─────────────────────────
    fn ui_settings(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        let acc = self.accent;
        egui::Window::new(RichText::new("Ajustes").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(470.0)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Zek Music v{}", env!("CARGO_PKG_VERSION")))
                            .strong()
                            .color(acc.hi),
                    );
                    if self.update_info.is_some() {
                        ui.colored_label(WARN, " • versão nova disponível");
                    }
                });
                ui.add_space(8.0);
                ui.label(RichText::new("Cor de destaque").strong().color(TEXT));
                ui.horizontal_wrapped(|ui| {
                    for (i, (a, _)) in PALETTES.iter().enumerate() {
                        let c = Color32::from_rgb(a[0], a[1], a[2]);
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                        ui.painter().rect_filled(r, 8.0, c);
                        let marcada = self.cfg.accent_custom.is_none() && self.cfg.accent_idx as usize == i;
                        if marcada {
                            ui.painter().rect_stroke(r, 8.0, stroke(2.5, Color32::WHITE));
                        } else if resp.hovered() {
                            ui.painter().rect_stroke(r, 8.0, stroke(2.0, lerp_color(c, Color32::WHITE, 0.5)));
                        }
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            acts.push(Act::SetAccent(i as u8));
                        }
                    }
                });
                egui::CollapsingHeader::new(RichText::new("Cor personalizada").color(MUTED))
                    .default_open(self.cfg.accent_custom.is_some())
                    .show_unindented(ui, |ui| {
                        let (mut vr, mut vg, mut vb) = match self.cfg.accent_custom {
                            Some(c) => (c[0] as i32, c[1] as i32, c[2] as i32),
                            None => (self.accent.base.r() as i32, self.accent.base.g() as i32, self.accent.base.b() as i32),
                        };
                        let mut mudou = false;
                        ui.horizontal(|ui| {
                            for (nome, v) in [("R", &mut vr), ("G", &mut vg), ("B", &mut vb)] {
                                ui.label(RichText::new(nome).color(MUTED));
                                if ui.add(egui::DragValue::new(v).range(0..=255).speed(1.0)).changed() {
                                    mudou = true;
                                }
                                ui.add_space(6.0);
                            }
                        });
                        if mudou {
                            acts.push(Act::SetAccentCustom(Some([vr as u8, vg as u8, vb as u8])));
                        }
                    });
                ui.add_space(8.0);
                if !is_store_build() {
                    let mut bandeira = self.cfg.check_updates;
                    if ui
                        .checkbox(&mut bandeira, "Avisar quando sair versão nova (consulta o GitHub 1 vez ao dia)")
                        .changed()
                    {
                        acts.push(Act::SetCheckUpdates(bandeira));
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Verificar agora").clicked() {
                            acts.push(Act::CheckUpdates);
                        }
                        if ui.button("Abrir pasta do registro").clicked() {
                            acts.push(Act::OpenDataDir);
                        }
                    });
                }
                ui.add_space(10.0);
                ui.label(RichText::new("Registro de erro (últimas linhas)").color(MUTED));
                let linhas = log_tail(8);
                egui::ScrollArea::vertical().max_height(96.0).show(ui, |ui| {
                    for l in linhas {
                        ui.label(RichText::new(l).monospace().size(10.5).color(MUTED));
                    }
                });
            });
        self.show_settings = open;
    }
}

impl eframe::App for ZekApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
        let mut acts: Vec<Act> = vec![];
        self.ui_player(ctx, &mut acts);
        self.ui_sidebar(ctx, &mut acts);
        self.ui_central(ctx, &mut acts);
        self.ui_folders(ctx, &mut acts);
        self.ui_settings(ctx, &mut acts);
        self.ui_expanded(ctx);
        for a in acts {
            self.handle(a);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_config();
    }
}

// ───────────────────────── Estilo ─────────────────────────
fn setup_style(ctx: &egui::Context, accent: Accent) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(7, 11, 22);
    v.faint_bg_color = PANEL;
    v.selection.bg_fill = accent.base;
    v.selection.stroke = stroke(1.0, Color32::WHITE);
    v.hyperlink_color = accent.hi;
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.fg_stroke = stroke(1.0, TEXT);
    v.widgets.noninteractive.bg_stroke = stroke(1.0, Color32::from_rgb(24, 34, 62));
    v.widgets.inactive.bg_fill = BTN;
    v.widgets.inactive.weak_bg_fill = BTN;
    v.widgets.inactive.fg_stroke = stroke(1.0, TEXT);
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.bg_fill = BTN_HOVER;
    v.widgets.hovered.weak_bg_fill = BTN_HOVER;
    v.widgets.hovered.fg_stroke = stroke(1.0, Color32::WHITE);
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.bg_fill = accent.base;
    v.widgets.active.weak_bg_fill = accent.base;
    v.widgets.active.fg_stroke = stroke(1.0, Color32::WHITE);
    let r = Rounding::same(10.0);
    v.widgets.noninteractive.rounding = r;
    v.widgets.inactive.rounding = r;
    v.widgets.hovered.rounding = r;
    v.widgets.active.rounding = r;
    v.widgets.open.rounding = r;
    v.window_rounding = Rounding::same(14.0);
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 8.0);
        s.spacing.button_padding = Vec2::new(14.0, 7.0);
    });
}

fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../packaging/zek-music.png")).ok()?;
    let img = img.thumbnail(256, 256).to_rgba8();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

fn build_viewport() -> egui::ViewportBuilder {
    let mut vp = egui::ViewportBuilder::default()
        .with_inner_size([1180.0, 740.0])
        .with_min_inner_size([980.0, 580.0])
        .with_title("Zek Music")
        .with_app_id("zek-music");
    if let Some(ic) = load_icon() {
        vp = vp.with_icon(ic);
    }
    vp
}

// ───────────────────────── Testes ─────────────────────────
#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn ordem_natural_dos_nomes() {
        let mut v = vec![
            PathBuf::from("10 - j.mp3"),
            PathBuf::from("2 - b.mp3"),
            PathBuf::from("1 - a.mp3"),
            PathBuf::from("Album/11 - k.mp3"),
        ];
        order_tracks(&mut v, &HashMap::new());
        assert_eq!(v, vec![
            PathBuf::from("1 - a.mp3"),
            PathBuf::from("2 - b.mp3"),
            PathBuf::from("10 - j.mp3"),
            PathBuf::from("Album/11 - k.mp3"),
        ]);
    }

    #[test]
    fn ordem_pelo_numero_da_faixa() {
        // etiquetas com número único: segue a ordem do álbum, não a do nome
        let mut v = vec![PathBuf::from("a.mp3"), PathBuf::from("b.mp3"), PathBuf::from("c.mp3")];
        let mut metas = HashMap::new();
        metas.insert(PathBuf::from("a.mp3"), Meta { track: Some(3), ..Default::default() });
        metas.insert(PathBuf::from("b.mp3"), Meta { track: Some(1), ..Default::default() });
        metas.insert(PathBuf::from("c.mp3"), Meta { track: Some(2), ..Default::default() });
        order_tracks(&mut v, &metas);
        assert_eq!(v, vec![PathBuf::from("b.mp3"), PathBuf::from("c.mp3"), PathBuf::from("a.mp3")]);
    }

    #[test]
    fn numero_repetido_usaa_natural() {
        // número repetido não dá para confiar: cai para a ordem natural
        let mut v = vec![PathBuf::from("a.mp3"), PathBuf::from("b.mp3")];
        let mut metas = HashMap::new();
        metas.insert(PathBuf::from("a.mp3"), Meta { track: Some(1), ..Default::default() });
        metas.insert(PathBuf::from("b.mp3"), Meta { track: Some(1), ..Default::default() });
        order_tracks(&mut v, &metas);
        assert_eq!(v, vec![PathBuf::from("a.mp3"), PathBuf::from("b.mp3")]);
    }

    #[test]
    fn aviso_de_versao_nova() {
        assert!(versao_mais_nova("v0.6.1", "0.6.0"));
        assert!(versao_mais_nova("v1.0.0", "0.99.9"));
        assert!(!versao_mais_nova("v0.6.0", "0.6.0"));
        assert!(!versao_mais_nova("v0.5.9", "0.6.0"));
    }

    #[test]
    fn selo_de_data() {
        assert_eq!(fmt_stamp(0), "1970-01-01 00:00:00");
        assert_eq!(fmt_stamp(1_767_225_600), "2026-01-01 00:00:00");
    }
}

#[cfg(not(windows))]
fn main() -> eframe::Result<()> {
    log_init();
    let opts = eframe::NativeOptions { viewport: build_viewport(), ..Default::default() };
    eframe::run_native("Zek Music", opts, Box::new(|cc| Ok(Box::new(ZekApp::new(cc)))))
}

#[cfg(windows)]
fn main() -> eframe::Result<()> {
    log_init();
    // 1ª tentativa: DirectX (funciona até em máquinas virtuais, sem placa de vídeo)
    let opts = eframe::NativeOptions { viewport: build_viewport(), renderer: eframe::Renderer::Wgpu, ..Default::default() };
    if eframe::run_native("Zek Music", opts, Box::new(|cc| Ok(Box::new(ZekApp::new(cc))))).is_ok() {
        return Ok(());
    }
    // 2ª tentativa: OpenGL
    let opts = eframe::NativeOptions { viewport: build_viewport(), renderer: eframe::Renderer::Glow, ..Default::default() };
    let r = eframe::run_native("Zek Music", opts, Box::new(|cc| Ok(Box::new(ZekApp::new(cc)))));
    if let Err(e) = &r {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Zek Music")
            .set_description(format!("Não foi possível abrir a janela do programa:\n\n{e}"))
            .show();
    }
    r
}
