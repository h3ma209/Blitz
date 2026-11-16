use dioxus::prelude::*;
use lucide_dioxus::{
    Activity, ArrowUpDown, Award, Bell, Bookmark, Boxes, Braces, Briefcase, Bug, Building,
    Calendar, Camera, ChartBar, Check, ChevronDown, ChevronRight, ChevronsDownUp, ChevronsUpDown,
    Clock, Cloud, Code,
    Columns2, Compass, Copy, Cpu, CreditCard, Database, Download, Eye, FileCode, FilePlus,
    FileText, Flag, Flame, FlaskConical, Folder, FolderPlus, Funnel, GitBranch,
    GitCommitHorizontal, GitMerge, Globe, HardDrive, Heart, House, Image, KeyRound, Laptop,
    Layers, Lightbulb, Link, Lock, Mail, MapPin, MessageSquare, Monitor, Network, Package,
    Palette, Pencil, Plug, Plus, Radio, Redo2, Rocket, Rows2, Rss, Save, Search, Send, Server,
    Settings, Share2, ShieldCheck, ShoppingCart, SlidersHorizontal, Smartphone, Sparkles, Star,
    Tag, Target, Terminal, Trash, Truck, Undo2, Upload, User, Users, Video, Wallet, WandSparkles,
    Webhook, Wifi, Wrench, X, Zap,
};

use super::DismissBackdrop;

/// Semantic tier (`"prod"`, `"stage"`, or `"local"`) derived from an environment's name.
pub fn env_tier(name: &str) -> &'static str {
    let lower = name.trim().to_lowercase();
    if lower.contains("prod") || lower.contains("live") {
        "prod"
    } else if lower.contains("stag")
        || lower.contains("uat")
        || lower.contains("qa")
        || lower.contains("test")
    {
        "stage"
    } else {
        "local"
    }
}

/// Curated environment icon keys paired with human-readable labels.
pub const ENV_ICON_CHOICES: &[(&str, &str)] = &[
    ("globe", "Globe"),
    ("laptop", "Local"),
    ("server", "Server"),
    ("cloud", "Cloud"),
    ("rocket", "Production"),
    ("flask", "Staging / Lab"),
    ("bug", "Debug / QA"),
    ("shield", "Shield"),
    ("lock", "Locked"),
    ("database", "Database"),
    ("terminal", "Terminal"),
    ("wrench", "Tools"),
    ("zap", "Blitz"),
    ("flame", "Hot"),
    ("sparkles", "Preview"),
    ("layers", "Stack"),
    ("boxes", "Containers"),
    ("cpu", "Compute"),
];

/// Crisp Lucide icon replacing legacy CSS `mask-image` `.action-icon-*` and emojis.
#[component]
pub fn ActionIcon(
    name: &'static str,
    #[props(default = 18)] size: usize,
    #[props(default = 2)] stroke_width: usize,
    #[props(default)] class: Option<String>,
    #[props(default)] style: Option<String>,
) -> Element {
    let cls = match class {
        Some(c) if !c.is_empty() => format!("lucide-action-icon {c}"),
        _ => "lucide-action-icon".to_string(),
    };

    match name {
        "import" => rsx! { Download { size, stroke_width, class: cls, style } },
        "export" => rsx! { Upload { size, stroke_width, class: cls, style } },
        "sort" => rsx! { ArrowUpDown { size, stroke_width, class: cls, style } },
        "add" => rsx! { Plus { size, stroke_width, class: cls, style } },
        "star" => rsx! { Star { size, stroke_width, class: cls, style } },
        "duplicate" | "copy" => rsx! { Copy { size, stroke_width, class: cls, style } },
        "request" => rsx! { FilePlus { size, stroke_width, class: cls, style } },
        "folder-new" => rsx! { FolderPlus { size, stroke_width, class: cls, style } },
        "edit" => rsx! { Pencil { size, stroke_width, class: cls, style } },
        "delete" => rsx! { Trash { size, stroke_width, class: cls, style } },
        "settings" => rsx! { Settings { size, stroke_width, class: cls, style } },
        "sliders" => rsx! { SlidersHorizontal { size, stroke_width, class: cls, style } },
        "save" => rsx! { Save { size, stroke_width, class: cls, style } },
        "split-col" => rsx! { Columns2 { size, stroke_width, class: cls, style } },
        "split-row" => rsx! { Rows2 { size, stroke_width, class: cls, style } },
        "theme" => rsx! { Palette { size, stroke_width, class: cls, style } },
        "bolt" => rsx! { Zap { size, stroke_width, class: cls, style } },
        "search" => rsx! { Search { size, stroke_width, class: cls, style } },
        "eye" => rsx! { Eye { size, stroke_width, class: cls, style } },
        "check" => rsx! { Check { size, stroke_width, class: cls, style } },
        "format" => rsx! { WandSparkles { size, stroke_width, class: cls, style } },
        "git-merge" => rsx! { GitMerge { size, stroke_width, class: cls, style } },
        "branch" => rsx! { GitBranch { size, stroke_width, class: cls, style } },
        "commit" => rsx! { GitCommitHorizontal { size, stroke_width, class: cls, style } },
        "filter" => rsx! { Funnel { size, stroke_width, class: cls, style } },
        "fold" => rsx! { ChevronsDownUp { size, stroke_width, class: cls, style } },
        "expand" => rsx! { ChevronsUpDown { size, stroke_width, class: cls, style } },
        "undo" => rsx! { Undo2 { size, stroke_width, class: cls, style } },
        "redo" => rsx! { Redo2 { size, stroke_width, class: cls, style } },
        "globe" => rsx! { Globe { size, stroke_width, class: cls, style } },
        "lock" => rsx! { Lock { size, stroke_width, class: cls, style } },
        "folder" => rsx! { Folder { size, stroke_width, class: cls, style } },
        "cloud" => rsx! { Cloud { size, stroke_width, class: cls, style } },
        "lightbulb" => rsx! { Lightbulb { size, stroke_width, class: cls, style } },
        "file" => rsx! { FileText { size, stroke_width, class: cls, style } },
        "users" => rsx! { Users { size, stroke_width, class: cls, style } },
        "braces" => rsx! { Braces { size, stroke_width, class: cls, style } },
        "layers" => rsx! { Layers { size, stroke_width, class: cls, style } },
        "chevron-right" => rsx! { ChevronRight { size, stroke_width, class: cls, style } },
        "chevron-down" => rsx! { ChevronDown { size, stroke_width, class: cls, style } },
        "close" => rsx! { X { size, stroke_width, class: cls, style } },
        _ => rsx! { Zap { size, stroke_width, class: cls, style } },
    }
}

#[component]
fn EnvGlyph(icon_key: String, #[props(default = 15)] size: usize) -> Element {
    let stroke_width = 2;
    match icon_key.as_str() {
        "globe" => rsx! { Globe { size, stroke_width } },
        "laptop" => rsx! { Laptop { size, stroke_width } },
        "server" => rsx! { Server { size, stroke_width } },
        "cloud" => rsx! { Cloud { size, stroke_width } },
        "rocket" => rsx! { Rocket { size, stroke_width } },
        "flask" => rsx! { FlaskConical { size, stroke_width } },
        "bug" => rsx! { Bug { size, stroke_width } },
        "shield" => rsx! { ShieldCheck { size, stroke_width } },
        "lock" => rsx! { Lock { size, stroke_width } },
        "database" => rsx! { Database { size, stroke_width } },
        "terminal" => rsx! { Terminal { size, stroke_width } },
        "wrench" => rsx! { Wrench { size, stroke_width } },
        "zap" => rsx! { Zap { size, stroke_width } },
        "flame" => rsx! { Flame { size, stroke_width } },
        "sparkles" => rsx! { Sparkles { size, stroke_width } },
        "layers" => rsx! { Layers { size, stroke_width } },
        "boxes" => rsx! { Boxes { size, stroke_width } },
        "cpu" => rsx! { Cpu { size, stroke_width } },
        _ => rsx! { Globe { size, stroke_width } },
    }
}

/// Renders an environment's custom Lucide icon or its default tier dot.
#[component]
pub fn EnvIcon(
    icon: Option<String>,
    #[props(default = "local")] tier: &'static str,
    #[props(default = 15)] size: usize,
) -> Element {
    match icon.filter(|s| !s.trim().is_empty()) {
        Some(key) => rsx! {
            span { class: "env-icon env-icon-{tier}", aria_hidden: "true",
                EnvGlyph { icon_key: key, size }
            }
        },
        None => rsx! {
            span { class: "env-indicator env-indicator-{tier}", aria_hidden: "true" }
        },
    }
}

/// Compact button + popover grid to pick or clear an environment's icon.
#[component]
pub fn EnvIconPicker(
    icon: Option<String>,
    tier: &'static str,
    on_select: EventHandler<Option<String>>,
) -> Element {
    let mut open = use_signal(|| false);
    let current = icon.clone().filter(|s| !s.trim().is_empty());

    rsx! {
        div { class: "env-icon-picker-wrap",
            if open() {
                DismissBackdrop {
                    z_index: 950,
                    on_dismiss: move |_| open.set(false),
                }
            }
            button {
                class: "env-icon-picker-btn",
                title: "Choose environment icon",
                aria_label: "Choose environment icon",
                onclick: move |_| open.set(!open()),
                EnvIcon { icon: current.clone(), tier, size: 16 }
                span { class: "env-icon-picker-caret", "▾" }
            }
            if open() {
                div { class: "env-icon-popover",
                    div { class: "env-icon-popover-head",
                        span { "Environment Icon" }
                        if current.is_some() {
                            button {
                                class: "env-icon-clear-btn",
                                onclick: move |_| {
                                    open.set(false);
                                    on_select.call(None);
                                },
                                "Use Dot"
                            }
                        }
                    }
                    div { class: "env-icon-grid",
                        for (key, label) in ENV_ICON_CHOICES.iter() {
                            {
                                let key_owned = (*key).to_string();
                                let is_selected = current.as_deref() == Some(*key);
                                rsx! {
                                    button {
                                        key: "{key}",
                                        class: if is_selected { "env-icon-cell active" } else { "env-icon-cell" },
                                        title: "{label}",
                                        aria_label: "{label}",
                                        onclick: move |_| {
                                            open.set(false);
                                            on_select.call(Some(key_owned.clone()));
                                        },
                                        EnvGlyph { icon_key: (*key).to_string(), size: 17 }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Curated collection icon choices (67 crisp Lucide icons; `"layers"` is the default).
pub const COLLECTION_ICON_CHOICES: &[(&str, &str)] = &[
    ("layers", "Default (Layers)"),
    ("globe", "Web / REST"),
    ("server", "Backend / API"),
    ("cloud", "Cloud"),
    ("database", "Database"),
    ("shield", "Security / Auth"),
    ("lock", "Private / Vault"),
    ("key", "API Keys"),
    ("zap", "Fast / Realtime"),
    ("webhook", "Webhooks"),
    ("send", "Requests"),
    ("activity", "Monitoring"),
    ("rocket", "Launch / Prod"),
    ("terminal", "CLI / Scripts"),
    ("code", "SDK / Code"),
    ("file-code", "OpenAPI / Spec"),
    ("braces", "JSON / Schema"),
    ("boxes", "Microservices"),
    ("package", "Packages / Releases"),
    ("cpu", "Compute / AI"),
    ("hard-drive", "Storage / Disk"),
    ("network", "Network / Mesh"),
    ("wifi", "Wireless / Edge"),
    ("plug", "Plugins / Integrations"),
    ("radio", "Streaming / Events"),
    ("rss", "Feeds / RSS"),
    ("git-branch", "Git / Branches"),
    ("bug", "Debug / QA"),
    ("wrench", "Internal Tools"),
    ("sliders", "Config / Admin"),
    ("settings", "System Settings"),
    ("search", "Search / Discovery"),
    ("flask", "Testing / Lab"),
    ("flame", "Core / Hot"),
    ("sparkles", "Experimental / AI"),
    ("user", "Users / Profile"),
    ("users", "Teams / Groups"),
    ("building", "Enterprise / Org"),
    ("briefcase", "Business / CRM"),
    ("credit-card", "Payments / Billing"),
    ("wallet", "Wallet / Fintech"),
    ("shopping-cart", "E-Commerce / Orders"),
    ("truck", "Logistics / Shipping"),
    ("mail", "Email / SMTP"),
    ("message-square", "Chat / Messaging"),
    ("bell", "Notifications / Alerts"),
    ("calendar", "Scheduling / Cron"),
    ("clock", "Time / SLA"),
    ("chart-bar", "Analytics / Metrics"),
    ("smartphone", "Mobile API"),
    ("monitor", "Desktop / Web App"),
    ("camera", "Media / Capture"),
    ("image", "Images / Assets"),
    ("video", "Video / Streaming"),
    ("map-pin", "Geo / Maps"),
    ("house", "Home / Portal"),
    ("link", "Links / URLs"),
    ("share", "Public / Share"),
    ("tag", "Tags / Catalog"),
    ("flag", "Feature Flags"),
    ("target", "Goals / Endpoints"),
    ("award", "Partner / Certified"),
    ("heart", "Favorites / Health"),
    ("star", "Featured"),
    ("bookmark", "Saved"),
    ("compass", "Explore"),
];

/// Curated collection color swatches (default is emerald `#10b981`).
pub const COLLECTION_COLOR_CHOICES: &[(&str, &str)] = &[
    ("#10b981", "Emerald (Default)"),
    ("#22c55e", "Green"),
    ("#14b8a6", "Teal"),
    ("#06b6d4", "Cyan"),
    ("#f59e0b", "Amber"),
    ("#f97316", "Orange"),
    ("#ef4444", "Red"),
    ("#ec4899", "Pink"),
    ("#a855f7", "Purple"),
    ("#6366f1", "Indigo"),
    ("#94a3b8", "Slate"),
    ("#e2e8f0", "Silver"),
];

pub const DEFAULT_COLLECTION_COLOR: &str = "#10b981";

#[component]
fn CollectionGlyph(icon_key: String, #[props(default = 16)] size: usize) -> Element {
    let stroke_width = 2;
    match icon_key.as_str() {
        "globe" => rsx! { Globe { size, stroke_width } },
        "server" => rsx! { Server { size, stroke_width } },
        "cloud" => rsx! { Cloud { size, stroke_width } },
        "database" => rsx! { Database { size, stroke_width } },
        "shield" => rsx! { ShieldCheck { size, stroke_width } },
        "lock" => rsx! { Lock { size, stroke_width } },
        "key" => rsx! { KeyRound { size, stroke_width } },
        "zap" => rsx! { Zap { size, stroke_width } },
        "webhook" => rsx! { Webhook { size, stroke_width } },
        "send" => rsx! { Send { size, stroke_width } },
        "activity" => rsx! { Activity { size, stroke_width } },
        "rocket" => rsx! { Rocket { size, stroke_width } },
        "terminal" => rsx! { Terminal { size, stroke_width } },
        "code" => rsx! { Code { size, stroke_width } },
        "file-code" => rsx! { FileCode { size, stroke_width } },
        "braces" => rsx! { Braces { size, stroke_width } },
        "boxes" => rsx! { Boxes { size, stroke_width } },
        "package" => rsx! { Package { size, stroke_width } },
        "cpu" => rsx! { Cpu { size, stroke_width } },
        "hard-drive" => rsx! { HardDrive { size, stroke_width } },
        "network" => rsx! { Network { size, stroke_width } },
        "wifi" => rsx! { Wifi { size, stroke_width } },
        "plug" => rsx! { Plug { size, stroke_width } },
        "radio" => rsx! { Radio { size, stroke_width } },
        "rss" => rsx! { Rss { size, stroke_width } },
        "git-branch" => rsx! { GitBranch { size, stroke_width } },
        "bug" => rsx! { Bug { size, stroke_width } },
        "wrench" => rsx! { Wrench { size, stroke_width } },
        "sliders" => rsx! { SlidersHorizontal { size, stroke_width } },
        "settings" => rsx! { Settings { size, stroke_width } },
        "search" => rsx! { Search { size, stroke_width } },
        "flask" => rsx! { FlaskConical { size, stroke_width } },
        "flame" => rsx! { Flame { size, stroke_width } },
        "sparkles" => rsx! { Sparkles { size, stroke_width } },
        "user" => rsx! { User { size, stroke_width } },
        "users" => rsx! { Users { size, stroke_width } },
        "building" => rsx! { Building { size, stroke_width } },
        "briefcase" => rsx! { Briefcase { size, stroke_width } },
        "credit-card" => rsx! { CreditCard { size, stroke_width } },
        "wallet" => rsx! { Wallet { size, stroke_width } },
        "shopping-cart" => rsx! { ShoppingCart { size, stroke_width } },
        "truck" => rsx! { Truck { size, stroke_width } },
        "mail" => rsx! { Mail { size, stroke_width } },
        "message-square" => rsx! { MessageSquare { size, stroke_width } },
        "bell" => rsx! { Bell { size, stroke_width } },
        "calendar" => rsx! { Calendar { size, stroke_width } },
        "clock" => rsx! { Clock { size, stroke_width } },
        "chart-bar" => rsx! { ChartBar { size, stroke_width } },
        "smartphone" => rsx! { Smartphone { size, stroke_width } },
        "monitor" => rsx! { Monitor { size, stroke_width } },
        "camera" => rsx! { Camera { size, stroke_width } },
        "image" => rsx! { Image { size, stroke_width } },
        "video" => rsx! { Video { size, stroke_width } },
        "map-pin" => rsx! { MapPin { size, stroke_width } },
        "house" => rsx! { House { size, stroke_width } },
        "link" => rsx! { Link { size, stroke_width } },
        "share" => rsx! { Share2 { size, stroke_width } },
        "tag" => rsx! { Tag { size, stroke_width } },
        "flag" => rsx! { Flag { size, stroke_width } },
        "target" => rsx! { Target { size, stroke_width } },
        "award" => rsx! { Award { size, stroke_width } },
        "heart" => rsx! { Heart { size, stroke_width } },
        "star" => rsx! { Star { size, stroke_width } },
        "bookmark" => rsx! { Bookmark { size, stroke_width } },
        "compass" => rsx! { Compass { size, stroke_width } },
        _ => rsx! { Layers { size, stroke_width } },
    }
}

/// Renders a collection's custom or default Lucide icon with optional custom color.
#[component]
pub fn CollectionIcon(
    icon: Option<String>,
    color: Option<String>,
    #[props(default = 16)] size: usize,
) -> Element {
    let key = icon
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "layers".to_string());
    let hex = color
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_COLLECTION_COLOR.to_string());
    rsx! {
        span {
            class: "collection-icon-badge",
            style: "color: {hex};",
            aria_hidden: "true",
            CollectionGlyph { icon_key: key, size }
        }
    }
}

/// Popover picker for selecting a collection's icon and color (or resetting to default).
#[component]
pub fn CollectionIconPicker(
    icon: Option<String>,
    color: Option<String>,
    on_select: EventHandler<(Option<String>, Option<String>)>,
) -> Element {
    let mut open = use_signal(|| false);
    let mut query = use_signal(String::new);
    let cur_icon = icon.clone().filter(|s| !s.trim().is_empty());
    let cur_color = color.clone().filter(|s| !s.trim().is_empty());
    let active_icon_key = cur_icon.as_deref().unwrap_or("layers");
    let active_color_hex = cur_color.as_deref().unwrap_or(DEFAULT_COLLECTION_COLOR);
    let q_lower = query().trim().to_lowercase();

    rsx! {
        div { class: "env-icon-picker-wrap",
            if open() {
                DismissBackdrop {
                    z_index: 950,
                    on_dismiss: move |_| open.set(false),
                }
            }
            button {
                r#type: "button",
                class: "env-icon-picker-btn",
                title: "Customize collection icon & color",
                aria_label: "Customize collection icon & color",
                onclick: move |_| open.set(!open()),
                CollectionIcon { icon: cur_icon.clone(), color: cur_color.clone(), size: 17 }
                span { class: "env-icon-picker-caret", "▾" }
            }
            if open() {
                div { class: "env-icon-popover col-icon-popover",
                    div { class: "env-icon-popover-head",
                        span { "Collection Icon ({COLLECTION_ICON_CHOICES.len()})" }
                        if cur_icon.is_some() || cur_color.is_some() {
                            button {
                                r#type: "button",
                                class: "env-icon-clear-btn",
                                onclick: move |_| {
                                    open.set(false);
                                    query.set(String::new());
                                    on_select.call((None, None));
                                },
                                "Reset Default"
                            }
                        }
                    }
                    input {
                        class: "col-icon-search",
                        r#type: "text",
                        placeholder: "Filter icons (e.g. auth, git, pay)…",
                        value: "{query}",
                        oninput: move |evt| query.set(evt.value()),
                    }
                    div { class: "env-icon-grid col-icon-grid",
                        for (key, label) in COLLECTION_ICON_CHOICES.iter().filter(|(k, l)| {
                            q_lower.is_empty()
                                || k.to_lowercase().contains(&q_lower)
                                || l.to_lowercase().contains(&q_lower)
                        }) {
                            {
                                let key_str = (*key).to_string();
                                let is_sel = active_icon_key == *key;
                                let next_color = cur_color.clone();
                                rsx! {
                                    button {
                                        key: "{key}",
                                        r#type: "button",
                                        class: if is_sel { "env-icon-cell active" } else { "env-icon-cell" },
                                        title: "{label}",
                                        aria_label: "{label}",
                                        style: if is_sel { format!("color: {active_color_hex};") } else { String::new() },
                                        onclick: move |_| {
                                            let next_icon = if key_str == "layers" { None } else { Some(key_str.clone()) };
                                            on_select.call((next_icon, next_color.clone()));
                                        },
                                        CollectionGlyph { icon_key: (*key).to_string(), size: 17 }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "env-icon-popover-head", style: "margin-top: 10px;",
                        span { "Icon Color" }
                    }
                    div { class: "col-color-row",
                        for (hex, label) in COLLECTION_COLOR_CHOICES.iter() {
                            {
                                let hex_str = (*hex).to_string();
                                let is_sel = active_color_hex.eq_ignore_ascii_case(hex);
                                let next_icon = cur_icon.clone();
                                rsx! {
                                    button {
                                        key: "{hex}",
                                        r#type: "button",
                                        class: if is_sel { "col-color-swatch active" } else { "col-color-swatch" },
                                        title: "{label}",
                                        aria_label: "{label}",
                                        style: "background: {hex};",
                                        onclick: move |_| {
                                            let c = if hex_str.eq_ignore_ascii_case(DEFAULT_COLLECTION_COLOR) {
                                                None
                                            } else {
                                                Some(hex_str.clone())
                                            };
                                            on_select.call((next_icon.clone(), c));
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
