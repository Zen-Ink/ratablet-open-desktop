use super::{
    connection_loop, is_auth_error, normalize_host, Args, ConnectionStatus, RotationMode,
    SharedState,
};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, StrokeKind, Vec2};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::Duration;

const LANGUAGE_KEY: &str = "language";
const ROTATION_KEY: &str = "rotation";
const HOST_KEY: &str = "ssh_target";
const CREDENTIAL_SERVICE: &str = "io.ratablet.app";

pub fn run(args: Args) -> eframe::Result {
    let password = keyring::Entry::new(CREDENTIAL_SERVICE, &args.host)
        .and_then(|entry| entry.get_password())
        .ok();
    let shared = Arc::new(SharedState::new(
        RotationMode::from_args(&args),
        args.host.clone(),
        password,
    ));
    let worker_args = args.clone();
    let worker_shared = Arc::clone(&shared);
    let worker = thread::spawn(move || connection_loop(worker_args, worker_shared));
    let quitting = Arc::new(AtomicBool::new(false));

    #[cfg(target_os = "linux")]
    let result = run_linux(args, Arc::clone(&shared), Arc::clone(&quitting));
    #[cfg(not(target_os = "linux"))]
    let result = run_panel(args, Arc::clone(&shared), Arc::clone(&quitting), false);

    shared.stop();
    let _ = worker.join();
    result
}

#[cfg(target_os = "linux")]
fn run_linux(args: Args, shared: Arc<SharedState>, quitting: Arc<AtomicBool>) -> eframe::Result {
    let (show_tx, show_rx) = mpsc::sync_channel(1);
    let panel_ctx = Arc::new(Mutex::new(None));
    let language = Language::detect();
    let tray = create_linux_tray(
        language,
        show_tx,
        Arc::clone(&panel_ctx),
        Arc::clone(&quitting),
    );
    let auto_close = tray.is_some();
    let mut first = true;
    while !quitting.load(Ordering::Relaxed) {
        if first {
            first = false;
        } else {
            if show_rx.recv().is_err() {
                break;
            }
        }
        run_panel(
            args.clone(),
            Arc::clone(&shared),
            Arc::clone(&quitting),
            Arc::clone(&panel_ctx),
            tray.clone(),
            auto_close,
        )?;
        *panel_ctx.lock().unwrap() = None;
        if !auto_close {
            break;
        }
    }
    Ok(())
}

fn run_panel(
    args: Args,
    shared: Arc<SharedState>,
    quitting: Arc<AtomicBool>,
    #[cfg(target_os = "linux")] panel_ctx: Arc<Mutex<Option<egui::Context>>>,
    #[cfg(target_os = "linux")] tray: Option<ksni::blocking::Handle<LinuxTray>>,
    auto_close: bool,
) -> eframe::Result {
    let icon = icon_rgba(32);
    let options = eframe::NativeOptions {
        centered: false,
        persist_window: false,
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_app_id("io.ratablet.app")
            .with_title("ratablet")
            .with_inner_size([440.0, 500.0])
            .with_resizable(false)
            .with_decorations(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_visible(true)
            .with_icon(Arc::new(egui::IconData {
                rgba: icon,
                width: 32,
                height: 32,
            })),
        ..Default::default()
    };
    #[cfg(target_os = "macos")]
    let options = {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        let mut options = options;
        options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_activation_policy(ActivationPolicy::Accessory);
        }));
        options
    };
    eframe::run_native(
        "ratablet",
        options,
        Box::new(move |cc| {
            #[cfg(target_os = "linux")]
            {
                *panel_ctx.lock().unwrap() = Some(cc.egui_ctx.clone());
            }
            Ok(Box::new(App::new(
                cc,
                args,
                shared,
                quitting,
                #[cfg(target_os = "linux")]
                tray.clone(),
                auto_close,
            )))
        }),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Device,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Language {
    English,
    Chinese,
}

impl Language {
    fn detect() -> Self {
        if sys_locale::get_locale()
            .is_some_and(|locale| locale.to_ascii_lowercase().starts_with("zh"))
        {
            Self::Chinese
        } else {
            Self::English
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "en" => Some(Self::English),
            "zh-CN" => Some(Self::Chinese),
            _ => None,
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Chinese => "zh-CN",
        }
    }
}

struct Text {
    device_tab: &'static str,
    settings_tab: &'static str,
    connected: &'static str,
    connecting: &'static str,
    waiting: &'static str,
    device: &'static str,
    rotation_status: &'static str,
    rotation_setting: &'static str,
    landscape: &'static str,
    automatic: &'static str,
    input_mapping: &'static str,
    pen_tip: &'static str,
    primary_click: &'static str,
    barrel_button: &'static str,
    barrel_action: &'static str,
    second_button: &'static str,
    second_action: &'static str,
    eraser: &'static str,
    eraser_action: &'static str,
    language: &'static str,
    english: &'static str,
    chinese: &'static str,
    ssh_target: &'static str,
    apply_target: &'static str,
    target_applied: &'static str,
    invalid_target: &'static str,
    password: &'static str,
    password_hint: &'static str,
    save_password: &'static str,
    forget_password: &'static str,
    password_saved: &'static str,
    password_forgotten: &'static str,
    credential_error: &'static str,
    authentication_required: &'static str,
    authentication_message: &'static str,
    save_and_retry: &'static str,
    #[cfg(target_os = "macos")]
    accessibility_required: &'static str,
    #[cfg(target_os = "macos")]
    accessibility_message: &'static str,
    #[cfg(target_os = "macos")]
    open_accessibility: &'static str,
    later: &'static str,
    popup_behavior: &'static str,
    quit: &'static str,
}

impl Text {
    fn for_language(language: Language) -> Self {
        match language {
            Language::English => Self {
                device_tab: "Device",
                settings_tab: "Settings",
                connected: "Connected",
                connecting: "Connecting…",
                waiting: "Waiting to reconnect…",
                device: "Device",
                rotation_status: "Rotation",
                rotation_setting: "Rotation setting",
                landscape: "Landscape (default)",
                automatic: "Automatic",
                input_mapping: "Input mapping",
                pen_tip: "Pen tip",
                primary_click: "Primary click",
                barrel_button: "Barrel button",
                barrel_action: "Stylus button 1",
                second_button: "Second button",
                second_action: "Stylus button 2",
                eraser: "Eraser",
                eraser_action: "Tablet eraser",
                language: "Language",
                english: "English",
                chinese: "简体中文",
                ssh_target: "SSH target",
                apply_target: "Apply and reconnect",
                target_applied: "SSH target applied.",
                invalid_target: "Enter an IP address, hostname, or user@host without spaces.",
                password: "SSH password",
                password_hint: "Keys are always tried first.",
                save_password: "Save password",
                forget_password: "Forget",
                password_saved: "Password saved in the system credential store.",
                password_forgotten: "Saved password removed.",
                credential_error: "Credential store error",
                authentication_required: "Authentication required",
                authentication_message:
                    "Key authentication failed. Enter the device password, save it, and retry.",
                save_and_retry: "Save and retry",
                #[cfg(target_os = "macos")]
                accessibility_required: "Accessibility permission required",
                #[cfg(target_os = "macos")]
                accessibility_message: "Open System Settings → Privacy & Security → Accessibility, then turn on the switch next to ratablet.",
                #[cfg(target_os = "macos")]
                open_accessibility: "Open Accessibility Settings",
                later: "Later",
                popup_behavior: "The window hides when it loses focus.",
                quit: "Quit ratablet",
            },
            Language::Chinese => Self {
                device_tab: "设备",
                settings_tab: "设置",
                connected: "已连接",
                connecting: "正在连接…",
                waiting: "等待重新连接…",
                device: "设备",
                rotation_status: "当前方向",
                rotation_setting: "旋转设置",
                landscape: "横屏（默认）",
                automatic: "自动旋转",
                input_mapping: "按键映射",
                pen_tip: "笔尖",
                primary_click: "主点击",
                barrel_button: "笔身键",
                barrel_action: "数位笔按键 1",
                second_button: "第二按键",
                second_action: "数位笔按键 2",
                eraser: "橡皮擦",
                eraser_action: "数位板橡皮擦",
                language: "语言",
                english: "English",
                chinese: "简体中文",
                ssh_target: "SSH 目标",
                apply_target: "应用并重连",
                target_applied: "SSH 目标已应用。",
                invalid_target: "请输入不含空格的 IP、主机名或 user@host。",
                password: "SSH 密码",
                password_hint: "程序始终优先尝试密钥认证。",
                save_password: "保存密码",
                forget_password: "忘记密码",
                password_saved: "密码已保存到系统凭据库。",
                password_forgotten: "已删除保存的密码。",
                credential_error: "凭据库错误",
                authentication_required: "需要认证",
                authentication_message: "密钥认证失败。请输入设备密码，保存后再重新连接。",
                save_and_retry: "保存并重试",
                #[cfg(target_os = "macos")]
                accessibility_required: "需要辅助功能权限",
                #[cfg(target_os = "macos")]
                accessibility_message:
                    "请打开“系统设置 → 隐私与安全性 → 辅助功能”，开启 ratablet 旁边的开关。",
                #[cfg(target_os = "macos")]
                open_accessibility: "打开辅助功能设置",
                later: "稍后",
                popup_behavior: "窗口失去焦点后会自动隐藏。",
                quit: "退出 ratablet",
            },
        }
    }
}

struct App {
    args: Args,
    shared: Arc<SharedState>,
    page: Page,
    language: Language,
    was_focused: bool,
    auto_close: bool,
    host_edit: String,
    password: String,
    credential_message: Option<(bool, String)>,
    dismissed_auth_error: Option<String>,
    announced_auth_error: Option<String>,
    #[cfg(target_os = "macos")]
    dismissed_accessibility: bool,
    quitting: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    tray: Option<ksni::blocking::Handle<LinuxTray>>,
    #[cfg(target_os = "linux")]
    _kde_hints: Option<KdeWindowHints>,
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    _tray: Option<tray_icon::TrayIcon>,
}

impl App {
    fn new(
        cc: &eframe::CreationContext<'_>,
        mut args: Args,
        shared: Arc<SharedState>,
        quitting: Arc<AtomicBool>,
        #[cfg(target_os = "linux")] tray: Option<ksni::blocking::Handle<LinuxTray>>,
        auto_close: bool,
    ) -> Self {
        egui_system_fonts::add_with_region(
            &cc.egui_ctx,
            egui_system_fonts::FontRegion::SimplifiedChinese,
            egui_system_fonts::FontStyle::Sans,
        );
        let language = cc
            .storage
            .and_then(|storage| storage.get_string(LANGUAGE_KEY))
            .as_deref()
            .and_then(Language::parse)
            .unwrap_or_else(Language::detect);
        let cli_rotation = RotationMode::from_args(&args);
        let rotation = if args.rotate.is_some() || args.auto_rotate {
            cli_rotation
        } else {
            cc.storage
                .and_then(|storage| storage.get_string(ROTATION_KEY))
                .as_deref()
                .and_then(parse_rotation)
                .unwrap_or(cli_rotation)
        };
        let host = if args.host_explicit {
            args.host.clone()
        } else {
            cc.storage
                .and_then(|storage| storage.get_string(HOST_KEY))
                .and_then(|host| normalize_host(&host).ok())
                .unwrap_or_else(|| args.host.clone())
        };
        if host != *shared.host.lock().unwrap() {
            let password = keyring::Entry::new(CREDENTIAL_SERVICE, &host)
                .and_then(|entry| entry.get_password())
                .ok();
            shared.set_host(host.clone(), password);
        }
        args.host = host.clone();
        shared.set_rotation_mode(rotation);
        let password = shared.password.lock().unwrap().clone().unwrap_or_default();
        #[cfg(target_os = "linux")]
        let kde_hints = KdeWindowHints::new(cc);
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        let tray = create_tray(&cc.egui_ctx, language, Arc::clone(&quitting));
        Self {
            args,
            shared,
            page: Page::Device,
            language,
            was_focused: false,
            auto_close,
            host_edit: host,
            password,
            credential_message: None,
            dismissed_auth_error: None,
            announced_auth_error: None,
            #[cfg(target_os = "macos")]
            dismissed_accessibility: false,
            quitting,
            #[cfg(target_os = "linux")]
            tray,
            #[cfg(target_os = "linux")]
            _kde_hints: kde_hints,
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            _tray: tray,
        }
    }

    fn device_page(&mut self, ui: &mut egui::Ui, text: &Text) {
        let status = self.shared.connection.lock().unwrap().clone();
        let (status_text, color) = match &status {
            ConnectionStatus::Connected(_) => (text.connected, Color32::from_rgb(55, 180, 110)),
            ConnectionStatus::Connecting => (text.connecting, Color32::from_rgb(232, 170, 45)),
            ConnectionStatus::Waiting(error) if is_auth_error(error) => (
                text.authentication_required,
                Color32::from_rgb(232, 100, 80),
            ),
            ConnectionStatus::Waiting(_) => (text.waiting, Color32::from_rgb(232, 100, 80)),
        };
        ui.horizontal(|ui| {
            ui.colored_label(color, "●");
            ui.heading(status_text);
        });
        ui.add_space(12.0);

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text.device).small().weak());
            match &status {
                ConnectionStatus::Connected(info) => {
                    ui.label(RichText::new(&info.model).size(21.0).strong());
                    let mut rotation = self.shared.effective_rotation.load(Ordering::Relaxed);
                    if rotation == u16::MAX {
                        rotation = self.shared.rotation_mode().rotation(info, rotation);
                    }
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        orientation_preview(ui, info.output_ranges(rotation));
                        ui.vertical(|ui| {
                            ui.label(RichText::new(text.rotation_status).small().weak());
                            ui.label(RichText::new(format!("{rotation}°")).size(24.0));
                        });
                    });
                }
                ConnectionStatus::Waiting(error) => {
                    ui.label(RichText::new(error).color(Color32::from_rgb(220, 90, 75)));
                }
                _ => {
                    ui.spinner();
                }
            }
        });

        ui.add_space(14.0);
        ui.label(RichText::new(text.rotation_setting).strong());
        let old = self.shared.rotation_mode();
        let mut selected = old;
        let auto_supported =
            !matches!(&status, ConnectionStatus::Connected(info) if !info.supports_auto_rotation());
        egui::ComboBox::from_id_salt("rotation")
            .selected_text(rotation_label(selected, text))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut selected, RotationMode::Landscape, text.landscape);
                ui.add_enabled_ui(auto_supported, |ui| {
                    ui.selectable_value(&mut selected, RotationMode::Auto, text.automatic);
                });
                for degrees in [0, 90, 180, 270] {
                    ui.selectable_value(
                        &mut selected,
                        RotationMode::Fixed(degrees),
                        format!("{degrees}°"),
                    );
                }
            });
        if selected != old {
            self.shared.set_rotation_mode(selected);
        }

        ui.add_space(16.0);
        ui.label(RichText::new(text.input_mapping).strong());
        egui::Grid::new("mapping").num_columns(2).show(ui, |ui| {
            mapping_row(ui, text.pen_tip, text.primary_click);
            mapping_row(ui, text.barrel_button, text.barrel_action);
            mapping_row(ui, text.second_button, text.second_action);
            mapping_row(ui, text.eraser, text.eraser_action);
        });
    }

    fn settings_page(&mut self, ui: &mut egui::Ui, text: &Text) {
        ui.heading(text.settings_tab);
        ui.add_space(16.0);
        ui.label(RichText::new(text.language).strong());
        #[cfg(target_os = "linux")]
        let old_language = self.language;
        egui::ComboBox::from_id_salt("language")
            .selected_text(match self.language {
                Language::English => text.english,
                Language::Chinese => text.chinese,
            })
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.language, Language::English, text.english);
                ui.selectable_value(&mut self.language, Language::Chinese, text.chinese);
            });
        #[cfg(target_os = "linux")]
        if self.language != old_language {
            if let Some(tray) = &self.tray {
                let (open, quit) = tray_text(self.language);
                tray.update(|tray| {
                    tray.open_label = open;
                    tray.quit_label = quit;
                });
            }
        }
        ui.add_space(16.0);
        ui.label(RichText::new(text.ssh_target).strong());
        ui.add(
            egui::TextEdit::singleline(&mut self.host_edit)
                .desired_width(ui.available_width())
                .hint_text("10.11.99.1"),
        );
        if ui.button(text.apply_target).clicked() {
            match normalize_host(&self.host_edit) {
                Ok(host) => {
                    let password = keyring::Entry::new(CREDENTIAL_SERVICE, &host)
                        .and_then(|entry| entry.get_password())
                        .ok();
                    self.host_edit.clone_from(&host);
                    self.args.host.clone_from(&host);
                    self.password = password.clone().unwrap_or_default();
                    self.shared.set_host(host, password);
                    self.credential_message = Some((true, text.target_applied.into()));
                }
                Err(_) => {
                    self.credential_message = Some((false, text.invalid_target.into()));
                }
            }
        }
        ui.add_space(12.0);
        ui.label(RichText::new(text.password).strong());
        ui.add(
            egui::TextEdit::singleline(&mut self.password)
                .password(true)
                .desired_width(ui.available_width()),
        );
        ui.label(RichText::new(text.password_hint).small().weak());
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.password.is_empty(),
                    egui::Button::new(text.save_password),
                )
                .clicked()
            {
                self.save_password(text);
            }
            if ui.button(text.forget_password).clicked() {
                let result = keyring::Entry::new(CREDENTIAL_SERVICE, &self.args.host)
                    .and_then(|entry| entry.delete_credential());
                self.password.clear();
                self.shared.set_password(None);
                self.credential_message = Some(match result {
                    Ok(()) | Err(keyring::Error::NoEntry) => (true, text.password_forgotten.into()),
                    Err(error) => (false, format!("{}: {error}", text.credential_error)),
                });
            }
        });
        if let Some((success, message)) = &self.credential_message {
            ui.colored_label(
                if *success {
                    Color32::from_rgb(55, 180, 110)
                } else {
                    Color32::from_rgb(220, 90, 75)
                },
                message,
            );
        }
        ui.add_space(16.0);
        ui.label(text.popup_behavior);
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            if ui.button(text.quit).clicked() {
                self.quitting.store(true, Ordering::Relaxed);
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn save_password(&mut self, text: &Text) -> bool {
        let result = keyring::Entry::new(CREDENTIAL_SERVICE, &self.args.host)
            .and_then(|entry| entry.set_password(&self.password));
        match result {
            Ok(()) => {
                self.shared.set_password(Some(self.password.clone()));
                self.credential_message = Some((true, text.password_saved.into()));
                true
            }
            Err(error) => {
                self.credential_message =
                    Some((false, format!("{}: {error}", text.credential_error)));
                false
            }
        }
    }

    fn auth_modal(&mut self, ctx: &egui::Context, text: &Text) {
        let error = match self.auth_error() {
            Some(error) => error,
            None => {
                self.dismissed_auth_error = None;
                return;
            }
        };
        if self.dismissed_auth_error.as_deref() == Some(&error) {
            return;
        }

        let mut dismiss = false;
        let response = egui::Modal::new(egui::Id::new("ssh_authentication")).show(ctx, |ui| {
            ui.set_max_width(330.0);
            ui.heading(text.authentication_required);
            ui.label(text.authentication_message);
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.password)
                    .password(true)
                    .desired_width(300.0),
            );
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !self.password.is_empty(),
                        egui::Button::new(text.save_and_retry),
                    )
                    .clicked()
                    && self.save_password(text)
                {
                    dismiss = true;
                }
                if ui.button(text.later).clicked() {
                    dismiss = true;
                }
            });
            if let Some((false, message)) = &self.credential_message {
                ui.colored_label(Color32::from_rgb(220, 90, 75), message);
            }
        });
        if dismiss || response.should_close() {
            self.dismissed_auth_error = Some(error);
        }
    }

    fn auth_error(&self) -> Option<String> {
        match self.shared.connection.lock().unwrap().clone() {
            ConnectionStatus::Waiting(error) if is_auth_error(&error) => Some(error),
            _ => None,
        }
    }

    #[cfg(target_os = "macos")]
    fn accessibility_modal(&mut self, ctx: &egui::Context, text: &Text) {
        if super::platform::has_post_event_access() {
            self.dismissed_accessibility = false;
            return;
        }
        if self.dismissed_accessibility {
            return;
        }

        let mut dismiss = false;
        let response = egui::Modal::new(egui::Id::new("macos_accessibility")).show(ctx, |ui| {
            ui.set_max_width(350.0);
            ui.heading(text.accessibility_required);
            ui.label(text.accessibility_message);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(text.open_accessibility).clicked() {
                    let _ = super::platform::open_accessibility_settings();
                }
                if ui.button(text.later).clicked() {
                    dismiss = true;
                }
            });
        });
        if dismiss || response.should_close() {
            self.dismissed_accessibility = true;
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (focused, close_requested) =
            ctx.input(|input| (input.viewport().focused, input.viewport().close_requested()));
        let auth_error = self.auth_error();
        if auth_error != self.announced_auth_error {
            self.announced_auth_error = auth_error.clone();
            if auth_error.is_some() && !self.auto_close {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                self.was_focused = false;
                ctx.request_repaint();
                return;
            }
        }
        if close_requested && !self.quitting.load(Ordering::Relaxed) && !self.auto_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.was_focused = false;
        } else if self.was_focused && focused == Some(false) {
            ctx.send_viewport_cmd(if self.auto_close {
                egui::ViewportCommand::Close
            } else {
                egui::ViewportCommand::Visible(false)
            });
            self.was_focused = false;
        } else if let Some(focused) = focused {
            self.was_focused = focused;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let text = Text::for_language(self.language);
        egui::Panel::left("tabs")
            .exact_size(58.0)
            .resizable(false)
            .frame(egui::Frame::side_top_panel(ui.style()).fill(Color32::from_gray(34)))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(12.0);
                    if tab_button(ui, "⌂", text.device_tab, self.page == Page::Device).clicked() {
                        self.page = Page::Device;
                    }
                    ui.add_space(8.0);
                    if tab_button(ui, "⚙", text.settings_tab, self.page == Page::Settings).clicked()
                    {
                        self.page = Page::Settings;
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(20.0))
            .show(ui, |ui| match self.page {
                Page::Device => self.device_page(ui, &text),
                Page::Settings => self.settings_page(ui, &text),
            });
        self.auth_modal(&ctx, &text);
        #[cfg(target_os = "macos")]
        self.accessibility_modal(&ctx, &text);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(LANGUAGE_KEY, self.language.code().into());
        storage.set_string(HOST_KEY, self.args.host.clone());
        storage.set_string(
            ROTATION_KEY,
            format_rotation(self.shared.rotation_mode()).into(),
        );
    }
}

fn mapping_row(ui: &mut egui::Ui, source: &str, target: &str) {
    ui.label(source);
    ui.label(RichText::new(target).weak());
    ui.end_row();
}

fn tab_button(ui: &mut egui::Ui, icon: &str, hint: &str, selected: bool) -> egui::Response {
    ui.add_sized(
        [42.0, 42.0],
        egui::Button::selectable(selected, RichText::new(icon).size(23.0)),
    )
    .on_hover_text(hint)
}

fn orientation_preview(ui: &mut egui::Ui, ranges: (i32, i32)) {
    let landscape = ranges.0 >= ranges.1;
    let size = if landscape {
        Vec2::new(90.0, 58.0)
    } else {
        Vec2::new(58.0, 90.0)
    };
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect(
        rect,
        5.0,
        Color32::from_gray(225),
        Stroke::new(1.5, Color32::from_gray(110)),
        StrokeKind::Inside,
    );
}

fn rotation_label(mode: RotationMode, text: &Text) -> String {
    match mode {
        RotationMode::Landscape => text.landscape.into(),
        RotationMode::Auto => text.automatic.into(),
        RotationMode::Fixed(degrees) => format!("{degrees}°"),
    }
}

fn format_rotation(mode: RotationMode) -> &'static str {
    match mode {
        RotationMode::Landscape => "landscape",
        RotationMode::Auto => "auto",
        RotationMode::Fixed(0) => "0",
        RotationMode::Fixed(90) => "90",
        RotationMode::Fixed(180) => "180",
        RotationMode::Fixed(270) => "270",
        RotationMode::Fixed(_) => unreachable!(),
    }
}

fn parse_rotation(value: &str) -> Option<RotationMode> {
    match value {
        "landscape" => Some(RotationMode::Landscape),
        "auto" => Some(RotationMode::Auto),
        "0" => Some(RotationMode::Fixed(0)),
        "90" => Some(RotationMode::Fixed(90)),
        "180" => Some(RotationMode::Fixed(180)),
        "270" => Some(RotationMode::Fixed(270)),
        _ => None,
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn show_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}

#[cfg(target_os = "linux")]
struct LinuxTray {
    show: mpsc::SyncSender<()>,
    panel_ctx: Arc<Mutex<Option<egui::Context>>>,
    quitting: Arc<AtomicBool>,
    open_label: &'static str,
    quit_label: &'static str,
}

#[cfg(target_os = "linux")]
impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        "ratablet".into()
    }

    fn title(&self) -> String {
        "ratablet".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let mut data = icon_rgba(32);
        for pixel in data.chunks_exact_mut(4) {
            pixel.rotate_right(1);
        }
        vec![ksni::Icon {
            width: 32,
            height: 32,
            data,
        }]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.request_show();
    }

    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        self.request_show();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: self.open_label.into(),
                activate: Box::new(LinuxTray::request_show),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: self.quit_label.into(),
                activate: Box::new(LinuxTray::request_quit),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(target_os = "linux")]
impl LinuxTray {
    fn request_show(&mut self) {
        if self.panel_ctx.lock().unwrap().is_none() {
            let _ = self.show.try_send(());
        }
    }

    fn request_quit(&mut self) {
        self.quitting.store(true, Ordering::Relaxed);
        if let Some(ctx) = self.panel_ctx.lock().unwrap().as_ref() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ctx.request_repaint();
        } else {
            let _ = self.show.try_send(());
        }
    }
}

#[cfg(target_os = "linux")]
fn create_linux_tray(
    language: Language,
    show: mpsc::SyncSender<()>,
    panel_ctx: Arc<Mutex<Option<egui::Context>>>,
    quitting: Arc<AtomicBool>,
) -> Option<ksni::blocking::Handle<LinuxTray>> {
    use ksni::blocking::TrayMethods;
    let (open_label, quit_label) = tray_text(language);
    match (LinuxTray {
        show,
        panel_ctx,
        quitting,
        open_label,
        quit_label,
    })
    .spawn()
    {
        Ok(handle) => Some(handle),
        Err(error) => {
            eprintln!("ratablet: system tray unavailable: {error}");
            None
        }
    }
}

#[cfg(target_os = "linux")]
struct KdeState;

#[cfg(target_os = "linux")]
impl
    wayland_client::Dispatch<
        wayland_client::protocol::wl_registry::WlRegistry,
        wayland_client::globals::GlobalListContents,
    > for KdeState
{
    fn event(
        _: &mut Self,
        _: &wayland_client::protocol::wl_registry::WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &wayland_client::globals::GlobalListContents,
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

#[cfg(target_os = "linux")]
type PlasmaShell =
    wayland_protocols_plasma::plasma_shell::client::org_kde_plasma_shell::OrgKdePlasmaShell;
#[cfg(target_os = "linux")]
type PlasmaSurface =
    wayland_protocols_plasma::plasma_shell::client::org_kde_plasma_surface::OrgKdePlasmaSurface;

#[cfg(target_os = "linux")]
impl wayland_client::Dispatch<PlasmaShell, ()> for KdeState {
    fn event(
        _: &mut Self,
        _: &PlasmaShell,
        _: <PlasmaShell as wayland_client::Proxy>::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

#[cfg(target_os = "linux")]
impl wayland_client::Dispatch<PlasmaSurface, ()> for KdeState {
    fn event(
        _: &mut Self,
        _: &PlasmaSurface,
        _: <PlasmaSurface as wayland_client::Proxy>::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

#[cfg(target_os = "linux")]
struct KdeWindowHints {
    connection: wayland_client::Connection,
    _queue: wayland_client::EventQueue<KdeState>,
    surface: PlasmaSurface,
    _shell: PlasmaShell,
}

#[cfg(target_os = "linux")]
impl KdeWindowHints {
    fn new(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        use raw_window_handle::{HasDisplayHandle as _, HasWindowHandle as _};
        use wayland_client::{backend::ObjectId, Proxy as _};
        use wayland_protocols_plasma::plasma_shell::client::org_kde_plasma_surface::Role;

        let raw_window_handle::RawDisplayHandle::Wayland(display) =
            cc.display_handle().ok()?.as_raw()
        else {
            return None;
        };
        let raw_window_handle::RawWindowHandle::Wayland(window) = cc.window_handle().ok()?.as_raw()
        else {
            return None;
        };
        // SAFETY: eframe owns both objects for the full lifetime of this App.
        let backend = unsafe {
            wayland_client::backend::Backend::from_foreign_display(display.display.as_ptr().cast())
        };
        let connection = wayland_client::Connection::from_backend(backend);
        let id = unsafe {
            ObjectId::from_ptr(
                wayland_client::protocol::wl_surface::WlSurface::interface(),
                window.surface.as_ptr().cast(),
            )
        }
        .ok()?;
        let wl_surface =
            wayland_client::protocol::wl_surface::WlSurface::from_id(&connection, id).ok()?;
        let (globals, queue) =
            wayland_client::globals::registry_queue_init::<KdeState>(&connection).ok()?;
        let qh = queue.handle();
        let shell: PlasmaShell = globals.bind(&qh, 1..=8, ()).ok()?;
        let surface = shell.get_surface(&wl_surface, &qh, ());
        if surface.version() >= 2 {
            surface.set_skip_taskbar(1);
        }
        if surface.version() >= 5 {
            surface.set_skip_switcher(1);
        }
        if surface.version() >= 8 {
            surface.set_role(Role::Appletpopup.into());
        }
        if surface.version() >= 7 {
            surface.open_under_cursor();
        }
        connection.flush().ok()?;
        Some(Self {
            connection,
            _queue: queue,
            surface,
            _shell: shell,
        })
    }
}

#[cfg(target_os = "linux")]
impl Drop for KdeWindowHints {
    fn drop(&mut self) {
        self.surface.destroy();
        let _ = self.connection.flush();
    }
}

#[cfg(target_os = "linux")]
fn tray_text(language: Language) -> (&'static str, &'static str) {
    match language {
        Language::English => ("Open", "Quit"),
        Language::Chinese => ("打开", "退出"),
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn create_tray(
    ctx: &egui::Context,
    _language: Language,
    _quitting: Arc<AtomicBool>,
) -> Option<tray_icon::TrayIcon> {
    use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    let event_ctx = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |event| {
        if matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
        ) {
            show_window(&event_ctx);
        }
    }));
    let icon = tray_icon::Icon::from_rgba(icon_rgba(32), 32, 32).ok()?;
    let builder = TrayIconBuilder::new()
        .with_tooltip("ratablet")
        .with_menu_on_left_click(false)
        .with_icon(icon);
    #[cfg(target_os = "macos")]
    let builder = builder.with_icon_as_template(true);
    match builder.build() {
        Ok(tray) => Some(tray),
        Err(error) => {
            eprintln!("ratablet: system tray unavailable: {error}");
            None
        }
    }
}

fn icon_rgba(size: u32) -> Vec<u8> {
    let mut rgba = vec![0; (size * size * 4) as usize];
    let set = |rgba: &mut [u8], x: u32, y: u32, color: [u8; 4]| {
        let offset = ((y * size + x) * 4) as usize;
        rgba[offset..offset + 4].copy_from_slice(&color);
    };
    let ink = [28, 155, 150, 255];
    for y in 5..27 {
        for x in 4..22 {
            if !(7..24).contains(&y) || !(7..19).contains(&x) {
                set(&mut rgba, x, y, ink);
            }
        }
    }
    for step in 0..19 {
        let x = 12 + step;
        let y = 25u32.saturating_sub(step);
        if x < size && y < size {
            set(&mut rgba, x, y, [245, 245, 245, 255]);
            if y + 1 < size {
                set(&mut rgba, x, y + 1, ink);
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_rotation_values_round_trip() {
        for mode in [
            RotationMode::Landscape,
            RotationMode::Auto,
            RotationMode::Fixed(0),
            RotationMode::Fixed(90),
            RotationMode::Fixed(180),
            RotationMode::Fixed(270),
        ] {
            assert_eq!(parse_rotation(format_rotation(mode)), Some(mode));
        }
    }
}
