use dioxus::prelude::*;
use lucide_dioxus::{
    Activity, Anchor, Archive, ArrowUpDown, Atom, Award, Bell, Binary, Blocks, BookOpen, Bookmark,
    Bot, Boxes, Braces, Brain, Briefcase, Bug, Building, Cable, Calendar, Camera, ChartBar,
    ChartPie, Check, ChevronDown, ChevronRight, ChevronsDownUp, ChevronsUpDown, CircleDollarSign,
    ClipboardList, Clock, Cloud, CloudCog, Code, Columns2, Command, Compass, Container, Cookie,
    Copy, Cpu, CreditCard, Crown, Database, DatabaseZap, Download, Eye, FileCode,
    FilePlus, FileText, Flag, Flame, FlaskConical, Folder, FolderGit2, FolderPlus, Funnel,
    Gamepad2, Gauge, Gift, GitBranch, GitCommitHorizontal, GitMerge, Globe, GraduationCap, Hammer,
    HardDrive, Heart, HeartPulse, House, Image, Inbox, KeyRound, KeySquare, Landmark, Laptop,
    Layers, LayoutGrid, Lightbulb, Link, ListChecks, Lock, Magnet, Mail, MapPin, Megaphone,
    MessageSquare, Microscope, Monitor, Moon, Navigation, Network, Newspaper, Package,
    Palette, Pencil, Plane, Plug, Plus, Puzzle, QrCode, Radar, Radio, Receipt, Redo2, Repeat,
    Rocket, Rows2, Rss, Satellite, Save, Scale, ScanLine, Search, Send, Server, Settings, Share2,
    ShieldAlert, ShieldCheck, ShoppingCart, SlidersHorizontal, Smartphone, Sparkles, Star,
    Stethoscope, Store, Sun, Tag, Target, Terminal, Trash, Trophy, Truck, Umbrella, Undo2,
    Upload, User, Users, Utensils, Vault, Video, Wallet, WandSparkles, Warehouse, Watch, Webhook,
    Wifi, Workflow, Wrench, X, Zap,
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

/// Renders an environment's custom Lucide icon or its default tier dot.
#[component]
pub fn EnvIcon(
    icon: Option<String>,
    #[props(default)] color: Option<String>,
    #[props(default = "local")] tier: &'static str,
    #[props(default = 15)] size: usize,
) -> Element {
    let custom_color = color.filter(|s| !s.trim().is_empty());
    match icon.filter(|s| !s.trim().is_empty()) {
        Some(key) => {
            rsx! {
                span { class: "env-icon env-icon-{tier}", aria_hidden: "true",
                    CollectionGlyph { icon_key: key, size }
                }
            }
        }
        None => {
            let style = custom_color
                .as_deref()
                .map(|c| format!("background: {c}; box-shadow: 0 0 6px {c}80;"))
                .unwrap_or_default();
            rsx! {
                span { class: "env-indicator env-indicator-{tier}", style: "{style}", aria_hidden: "true" }
            }
        }
    }
}

/// Shared searchable icon + color swatch popover used by Environment & Collection pickers.
#[component]
fn IconColorPopover(
    title: String,
    reset_label: String,
    default_icon_key: Option<&'static str>,
    default_color_hex: Option<&'static str>,
    cur_icon: Option<String>,
    cur_color: Option<String>,
    mut query: Signal<String>,
    on_reset: EventHandler<()>,
    on_select: EventHandler<(Option<String>, Option<String>)>,
) -> Element {
    let q_lower = query().trim().to_lowercase();
    let active_icon_key = cur_icon.as_deref().or(default_icon_key).unwrap_or("");
    let active_color_hex = cur_color.as_deref().or(default_color_hex).unwrap_or("");

    rsx! {
        div { class: "env-icon-popover col-icon-popover",
            div { class: "env-icon-popover-head",
                span { "{title}" }
                if cur_icon.is_some() || cur_color.is_some() {
                    button {
                        r#type: "button",
                        class: "env-icon-clear-btn",
                        onclick: move |_| on_reset.call(()),
                        "{reset_label}"
                    }
                }
            }
            input {
                class: "col-icon-search",
                r#type: "text",
                placeholder: "Filter icons (e.g. auth, git, cloud)…",
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
                        let sel_color = if active_color_hex.is_empty() {
                            DEFAULT_COLLECTION_COLOR
                        } else {
                            active_color_hex
                        };
                        rsx! {
                            button {
                                key: "{key}",
                                r#type: "button",
                                class: if is_sel { "env-icon-cell active" } else { "env-icon-cell" },
                                title: "{label}",
                                aria_label: "{label}",
                                style: if is_sel { format!("color: {sel_color};") } else { String::new() },
                                onclick: move |_| {
                                    let next_icon = if default_icon_key == Some(key_str.as_str()) {
                                        None
                                    } else {
                                        Some(key_str.clone())
                                    };
                                    on_select.call((next_icon, next_color.clone()));
                                },
                                CollectionGlyph { icon_key: (*key).to_string(), size: 17 }
                            }
                        }
                    }
                }
            }
            div { class: "env-icon-popover-head", style: "margin-top: 10px;",
                span { "Color Label" }
            }
            div { class: "col-color-row",
                for (hex, label) in COLLECTION_COLOR_CHOICES.iter() {
                    {
                        let hex_str = (*hex).to_string();
                        let is_sel = !active_color_hex.is_empty() && active_color_hex.eq_ignore_ascii_case(hex);
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
                                    let c = if default_color_hex.is_some_and(|d| hex_str.eq_ignore_ascii_case(d)) {
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

/// Compact button + searchable popover to pick or clear an environment's icon and color label.
#[component]
pub fn EnvIconPicker(
    icon: Option<String>,
    color: Option<String>,
    tier: &'static str,
    on_select: EventHandler<(Option<String>, Option<String>)>,
) -> Element {
    let mut open = use_signal(|| false);
    let mut query = use_signal(String::new);
    let cur_icon = icon.clone().filter(|s| !s.trim().is_empty());
    let cur_color = color.clone().filter(|s| !s.trim().is_empty());

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
                title: "Customize environment icon & color",
                aria_label: "Customize environment icon & color",
                onclick: move |_| open.set(!open()),
                EnvIcon { icon: cur_icon.clone(), color: cur_color.clone(), tier, size: 16 }
                span { class: "env-icon-picker-caret", "▾" }
            }
            if open() {
                IconColorPopover {
                    title: format!("Environment Icon ({})", COLLECTION_ICON_CHOICES.len()),
                    reset_label: "Reset Dot".to_string(),
                    default_icon_key: None,
                    default_color_hex: None,
                    cur_icon,
                    cur_color,
                    query,
                    on_reset: move |_| {
                        open.set(false);
                        query.set(String::new());
                        on_select.call((None, None));
                    },
                    on_select,
                }
            }
        }
    }
}

/// Curated collection icon choices (`"layers"` is the default).
pub const COLLECTION_ICON_CHOICES: &[(&str, &str)] = &[
    ("layers", "Default (Layers)"),
    ("globe", "Web / REST"),
    ("server", "Backend / API"),
    ("cloud", "Cloud"),
    ("cloud-cog", "Cloud Ops / Infra"),
    ("database", "Database"),
    ("database-zap", "Cache / Fast DB"),
    ("shield", "Security / Auth"),
    ("lock", "Private / Vault"),
    ("vault", "Secrets / Vault"),
    ("key", "API Keys"),
    ("key-square", "Tokens / OAuth"),
    ("zap", "Fast / Realtime"),
    ("webhook", "Webhooks"),
    ("send", "Requests"),
    ("activity", "Monitoring"),
    ("heart-pulse", "Health Checks"),
    ("rocket", "Launch / Prod"),
    ("terminal", "CLI / Scripts"),
    ("command", "Shortcuts / Macros"),
    ("code", "SDK / Code"),
    ("file-code", "OpenAPI / Spec"),
    ("file-text", "Docs / Specs"),
    ("braces", "JSON / Schema"),
    ("binary", "Binary / Protobuf"),
    ("boxes", "Microservices"),
    ("blocks", "Modules / Blocks"),
    ("package", "Packages / Releases"),
    ("archive", "Archive / Backups"),
    ("cpu", "Compute / AI"),
    ("atom", "GraphQL / Core"),
    ("hard-drive", "Storage / Disk"),
    ("network", "Network / Mesh"),
    ("cable", "Sockets / Streams"),
    ("satellite", "Satellite / Telemetry"),
    ("wifi", "Wireless / Edge"),
    ("plug", "Plugins / Integrations"),
    ("puzzle", "Extensions / Addons"),
    ("radio", "Streaming / Events"),
    ("rss", "Feeds / RSS"),
    ("git-branch", "Git / Branches"),
    ("folder-git", "Repos / Source"),
    ("bug", "Debug / QA"),
    ("wrench", "Internal Tools"),
    ("cog", "Engine / Workers"),
    ("sliders", "Config / Admin"),
    ("settings", "System Settings"),
    ("search", "Search / Discovery"),
    ("scan-line", "OCR / Scanners"),
    ("qr-code", "QR / Mobile Auth"),
    ("flask", "Testing / Lab"),
    ("microscope", "Diagnostics / Trace"),
    ("flame", "Core / Hot"),
    ("sparkles", "Experimental / AI"),
    ("user", "Users / Profile"),
    ("users", "Teams / Groups"),
    ("building", "Enterprise / Org"),
    ("landmark", "Banking / Gov"),
    ("briefcase", "Business / CRM"),
    ("store", "Storefront / Retail"),
    ("credit-card", "Payments / Billing"),
    ("wallet", "Wallet / Fintech"),
    ("dollar", "Pricing / Ledger"),
    ("receipt", "Invoices / Receipts"),
    ("shopping-cart", "E-Commerce / Orders"),
    ("truck", "Logistics / Shipping"),
    ("warehouse", "Inventory / Warehouse"),
    ("mail", "Email / SMTP"),
    ("message-square", "Chat / Messaging"),
    ("megaphone", "Campaigns / Broadcast"),
    ("bell", "Notifications / Alerts"),
    ("calendar", "Scheduling / Cron"),
    ("clock", "Time / SLA"),
    ("watch", "Wearables / IoT"),
    ("chart-bar", "Analytics / Metrics"),
    ("chart-pie", "Reports / BI"),
    ("clipboard-list", "Tasks / Audit"),
    ("list-checks", "Checklists / QA"),
    ("book-open", "Docs / Guides"),
    ("newspaper", "News / Changelog"),
    ("graduation-cap", "Edu / Onboarding"),
    ("stethoscope", "Healthcare / FHIR"),
    ("utensils", "Food / Delivery"),
    ("plane", "Travel / Flights"),
    ("gamepad", "Gaming / LiveOps"),
    ("smartphone", "Mobile API"),
    ("monitor", "Desktop / Web App"),
    ("camera", "Media / Capture"),
    ("image", "Images / Assets"),
    ("video", "Video / Streaming"),
    ("map-pin", "Geo / Maps"),
    ("navigation", "Routing / GPS"),
    ("house", "Home / Portal"),
    ("link", "Links / URLs"),
    ("share", "Public / Share"),
    ("anchor", "Webhooks / Anchors"),
    ("magnet", "Ingestion / Leads"),
    ("repeat", "Sync / Retries"),
    ("scale", "Legal / Compliance"),
    ("tag", "Tags / Catalog"),
    ("flag", "Feature Flags"),
    ("target", "Goals / Endpoints"),
    ("award", "Partner / Certified"),
    ("trophy", "Milestones / SLA"),
    ("crown", "VIP / Enterprise"),
    ("gift", "Rewards / Promos"),
    ("heart", "Favorites / Health"),
    ("star", "Featured"),
    ("bookmark", "Saved"),
    ("compass", "Explore"),
    ("laptop", "Local / Dev"),
    ("bot", "AI / Bot"),
    ("brain", "ML / Intelligence"),
    ("container", "Docker / Containers"),
    ("cookie", "Cookies / Session"),
    ("gauge", "Performance / Metrics"),
    ("hammer", "Build / Tooling"),
    ("inbox", "Queue / Inbox"),
    ("layout-grid", "Grid / Modules"),
    ("palette", "Design / Themes"),
    ("sun", "Day / Light"),
    ("moon", "Night / Batch"),
    ("umbrella", "Insurance / Protection"),
    ("radar", "Telemetry / Scan"),
    ("shield-alert", "Alerts / WAF"),
    ("workflow", "Pipelines / Flows"),
    ("git-merge", "Merge / Release"),
    ("git-commit", "Commits / History"),
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
        "laptop" => rsx! { Laptop { size, stroke_width } },
        "bot" => rsx! { Bot { size, stroke_width } },
        "brain" => rsx! { Brain { size, stroke_width } },
        "container" => rsx! { Container { size, stroke_width } },
        "cookie" => rsx! { Cookie { size, stroke_width } },
        "gauge" => rsx! { Gauge { size, stroke_width } },
        "hammer" => rsx! { Hammer { size, stroke_width } },
        "inbox" => rsx! { Inbox { size, stroke_width } },
        "layout-grid" => rsx! { LayoutGrid { size, stroke_width } },
        "radar" => rsx! { Radar { size, stroke_width } },
        "shield-alert" => rsx! { ShieldAlert { size, stroke_width } },
        "workflow" => rsx! { Workflow { size, stroke_width } },
        "git-merge" => rsx! { GitMerge { size, stroke_width } },
        "git-commit" => rsx! { GitCommitHorizontal { size, stroke_width } },
        "cloud-cog" => rsx! { CloudCog { size, stroke_width } },
        "database-zap" => rsx! { DatabaseZap { size, stroke_width } },
        "vault" => rsx! { Vault { size, stroke_width } },
        "key-square" => rsx! { KeySquare { size, stroke_width } },
        "heart-pulse" => rsx! { HeartPulse { size, stroke_width } },
        "command" => rsx! { Command { size, stroke_width } },
        "file-text" => rsx! { FileText { size, stroke_width } },
        "binary" => rsx! { Binary { size, stroke_width } },
        "blocks" => rsx! { Blocks { size, stroke_width } },
        "archive" => rsx! { Archive { size, stroke_width } },
        "atom" => rsx! { Atom { size, stroke_width } },
        "cable" => rsx! { Cable { size, stroke_width } },
        "satellite" => rsx! { Satellite { size, stroke_width } },
        "puzzle" => rsx! { Puzzle { size, stroke_width } },
        "folder-git" => rsx! { FolderGit2 { size, stroke_width } },
        "cog" => rsx! { Settings { size, stroke_width } },
        "scan-line" => rsx! { ScanLine { size, stroke_width } },
        "qr-code" => rsx! { QrCode { size, stroke_width } },
        "microscope" => rsx! { Microscope { size, stroke_width } },
        "landmark" => rsx! { Landmark { size, stroke_width } },
        "store" => rsx! { Store { size, stroke_width } },
        "dollar" => rsx! { CircleDollarSign { size, stroke_width } },
        "receipt" => rsx! { Receipt { size, stroke_width } },
        "warehouse" => rsx! { Warehouse { size, stroke_width } },
        "megaphone" => rsx! { Megaphone { size, stroke_width } },
        "watch" => rsx! { Watch { size, stroke_width } },
        "chart-pie" => rsx! { ChartPie { size, stroke_width } },
        "clipboard-list" => rsx! { ClipboardList { size, stroke_width } },
        "list-checks" => rsx! { ListChecks { size, stroke_width } },
        "book-open" => rsx! { BookOpen { size, stroke_width } },
        "newspaper" => rsx! { Newspaper { size, stroke_width } },
        "graduation-cap" => rsx! { GraduationCap { size, stroke_width } },
        "stethoscope" => rsx! { Stethoscope { size, stroke_width } },
        "utensils" => rsx! { Utensils { size, stroke_width } },
        "plane" => rsx! { Plane { size, stroke_width } },
        "gamepad" => rsx! { Gamepad2 { size, stroke_width } },
        "navigation" => rsx! { Navigation { size, stroke_width } },
        "anchor" => rsx! { Anchor { size, stroke_width } },
        "magnet" => rsx! { Magnet { size, stroke_width } },
        "repeat" => rsx! { Repeat { size, stroke_width } },
        "scale" => rsx! { Scale { size, stroke_width } },
        "trophy" => rsx! { Trophy { size, stroke_width } },
        "crown" => rsx! { Crown { size, stroke_width } },
        "gift" => rsx! { Gift { size, stroke_width } },
        "palette" => rsx! { Palette { size, stroke_width } },
        "sun" => rsx! { Sun { size, stroke_width } },
        "moon" => rsx! { Moon { size, stroke_width } },
        "umbrella" => rsx! { Umbrella { size, stroke_width } },
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
                IconColorPopover {
                    title: format!("Collection Icon ({})", COLLECTION_ICON_CHOICES.len()),
                    reset_label: "Reset Default".to_string(),
                    default_icon_key: Some("layers"),
                    default_color_hex: Some(DEFAULT_COLLECTION_COLOR),
                    cur_icon,
                    cur_color,
                    query,
                    on_reset: move |_| {
                        open.set(false);
                        query.set(String::new());
                        on_select.call((None, None));
                    },
                    on_select,
                }
            }
        }
    }
}
