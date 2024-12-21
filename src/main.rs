use eframe::{egui, CreationContext};
use egui::Order;
use serialport::{available_ports, SerialPort, SerialPortType};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const KNOWN_MANUFACTURER: &str = "Espressif"; // Vervang door de daadwerkelijke fabrikant die je verwacht

#[derive(Default)]
struct ConnectionInfo {
    port_path: String,
    baudrate: u32,
    is_connected: bool,
}

impl ConnectionInfo {
    fn new(port_path: String, baudrate: u32) -> Self {
        ConnectionInfo {
            port_path,
            baudrate,
            is_connected: true,
        }
    }
}


fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_app_id("metalstream").with_inner_size([720.0, 480.0]),

        ..Default::default()
    };

    eframe::run_native(
        "Hardware Connection Example",
        options,
        Box::new(|cc| Ok(Box::new(MyApp::new(cc)))),
    )
}

#[derive(Default)]
struct MyApp {
    is_connected: bool,
    show_connection_window: bool,
    connection_info: Option<ConnectionInfo>,
}

impl MyApp {
    fn new(cc: &CreationContext) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        re_ui::apply_style_and_install_loaders(&cc.egui_ctx);
        egui_material_icons::initialize(&cc.egui_ctx);

        Self {
            ..Default::default()
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.show_connection_window {
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("connection_window"),
                egui::ViewportBuilder::default()
                    .with_title("Hardware Connection")
                    .with_inner_size([300.0, 150.0]),
                |ctx, _class| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        // ui.label(&self.connection_message);

                        if ui.button("Opnieuw proberen").clicked() {
                            if scan_and_connect().is_ok() {
                                self.show_connection_window = false;
                                self.is_connected = true;
                            }
                        }

                        if ui.button("Sluiten").clicked() {
                            self.show_connection_window = false
                        }
                    });
                },
            );
        }

        if self.connection_info.is_none() {
            match scan_and_connect() {
                Ok(connection_info) => {
                    self.connection_info = Some(connection_info);
                    self.show_connection_window = false;
                }
                Err(err) => {
                    // self.connection_message = err;
                    self.show_connection_window = true;
                }
            }
        }

        egui::TopBottomPanel::bottom("bottom_panel")
            .frame(re_ui::DesignTokens::bottom_panel_frame())
            .show(ctx, |ui| {
                if let Some(ref connection_info) = self.connection_info {
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new(format!("{} {}", egui_material_icons::icons::ICON_POWER, connection_info.port_path)).size(10.0)).clicked() {
                            // Acties wanneer de knop wordt geklikt
                            println!("USB icon button clicked!");
                        }
                        let _ = ui.label(egui::RichText::new(connection_info.baudrate.to_string()).size(10.0));
                    });
                }
            });

        
            egui::SidePanel::left("left_panel")
            .width_range(150.0..=300.0)
            .resizable(true)
            .frame(egui::Frame {
                fill: ctx.style().visuals.panel_fill,
                inner_margin: egui::Margin::same(5.0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add(egui::Image::new(egui::include_image!("../assets/Logo_Full_White_Transparent.png")).max_width(100.0));
                });

                ui.button("a");
            });

            egui::CentralPanel::default().frame(egui::Frame {
                fill: ctx.style().visuals.panel_fill,
                inner_margin: egui::Margin::same(5.0),
                ..Default::default()
            }).show(ctx, |ui| {
                ui.heading("Dit is de root applicatie.");

                if let Some(ref connection_info) = self.connection_info {
                    ui.label(format!("Verbonden met: {}", connection_info.port_path));
                    ui.label(format!("Baudrate: {}", connection_info.baudrate));    
                } else {
                    ui.label("Niet verbonden met hardware.");
                }
            });
    }
}

fn scan_and_connect() -> Result<ConnectionInfo, String> {
    let ports = available_ports().map_err(|e| format!("Error listing ports: {}", e))?;
    for port in ports {
        if let SerialPortType::UsbPort(usb) = &port.port_type {
            if let Some(manufacturer) = &usb.manufacturer {
                if manufacturer == KNOWN_MANUFACTURER {
                    return try_connect(&port.port_name);
                }
            }
        }
    }
    Err("Geen geschikte hardware gevonden.".to_string())
}

fn try_connect(port_name: &str) -> Result<ConnectionInfo, String> {
    let port = serialport::new(port_name, 9600)
        .timeout(Duration::from_secs(2))
        .open();

    match port {
        Ok(mut port) => {
            // Bij succes, retourneer de ConnectionInfo
            let baudrate = 9600; // Dit kan worden ingesteld op basis van wat je nodig hebt
            Ok(ConnectionInfo::new(port_name.to_string(), baudrate))
        }
        Err(e) => Err(format!("Kon poort {} niet openen: {}", port_name, e)),
    }
}
