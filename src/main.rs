use eframe::{egui, CreationContext};
use egui::Order;
use re_ui::UiExt;
use serialport::{available_ports, SerialPort, SerialPortBuilder, SerialPortType};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::sync::mpsc::{Receiver, channel};

const KNOWN_MANUFACTURER: &str = "Espressif"; // Vervang door de daadwerkelijke fabrikant die je verwacht

struct ConnectionInfo {
    port_path: String,
    baudrate: u32,
    is_connected: bool,
    connection: Box<dyn SerialPort>,
}

impl ConnectionInfo {
    fn new(port_path: String, baudrate: u32, connection: Box<dyn SerialPort>) -> Self {
        ConnectionInfo {
            port_path,
            baudrate,
            is_connected: true,
            connection
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
    logs: VecDeque<String>,  // Optimaliseer logs-opslag
    log_receiver: Option<Receiver<String>>,  // Voor inkomende logs
}

impl MyApp {
    fn new(cc: &CreationContext) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        re_ui::apply_style_and_install_loaders(&cc.egui_ctx);
        egui_material_icons::initialize(&cc.egui_ctx);

        let (sender, receiver) = channel();
        let port_name = "/dev/ttyACM0".to_string(); // Vervang dit met de echte poortnaam

        std::thread::spawn(move || {
            let port = serialport::new(&port_name, 115200)
                .timeout(std::time::Duration::from_millis(100))
                .open()
                .expect("Failed to open port");

            let mut port = port;
            let mut buffer = vec![0; 1024];
            loop {
                match port.read(&mut buffer) {
                    Ok(size) if size > 0 => {
                        let received = String::from_utf8_lossy(&buffer[..size]).to_string();
                        sender.send(received).ok();
                    },
                    Ok(_) => {
                        println!("Ok(_):")
                    },
                    Err(bla) => {
                        println!("Error on thread: {}", bla);
                    }, // Negeer timeouts
                }
            }
        });

        Self {
            logs: VecDeque::with_capacity(100),
            log_receiver: Some(receiver),
            ..Default::default()
        }
    }
    fn spawn_serial_thread() {
        
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // if !self.is_connected {
        //     match scan_and_connect() {
        //         Ok(connection_info) => {
        //             self.connection_info = Some(connection_info);
        //             self.is_connected = true;
        //         }
        //         Err(err) => {
        //             println!("Error state scan and connect");
        //             self.show_connection_window = true;
        //         }
        //     }
        // }

        // let top_bar_style = ctx.top_bar_style(false);

        egui::TopBottomPanel::top("top_bar")
            .frame(re_ui::DesignTokens::top_panel_frame())
            // .exact_height(top_bar_style.height)
            .show(ctx, |ui| {
                #[cfg(not(target_arch = "wasm32"))]
                if !re_ui::NATIVE_WINDOW_BAR {
                    // Interact with background first, so that buttons in the top bar gets input priority
                    // (last added widget has priority for input).
                    let title_bar_response = ui.interact(
                        ui.max_rect(),
                        ui.id().with("background"),
                        egui::Sense::click(),
                    );
                    if title_bar_response.double_clicked() {
                        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    } else if title_bar_response.is_pointer_button_down_on() {
                        // TODO(emilk): This should probably only run on `title_bar_response.drag_started_by(PointerButton::Primary)`,
                        // see https://github.com/emilk/egui/pull/4656
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                }

                egui::menu::bar(ui, |ui| {
                    // ui.set_height(top_bar_style.height);
                    // ui.add_space(top_bar_style.indent);

                    ui.menu_button("File", |ui| {
                        
                    });
                });
            });

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

                ui.horizontal_wrapped(|ui| {
                    ui.button("Start");
                    ui.button("Stop");
                    ui.button("Calibrate");
                });

                re_ui::list_item::list_item_scope(ui, "left_panel", |ui| {
                ui.section_collapsing_header("bla")
                    .show(ui, |ui| {
                        ui.label("Some data here");
                    });
                });
            });

            egui::CentralPanel::default().frame(egui::Frame {
                fill: ctx.style().visuals.panel_fill,
                inner_margin: egui::Margin::same(5.0),
                ..Default::default()
            }).show(ctx, |ui| {
                ui.heading("Dit is de root applicatie.");

                if let Some(receiver) = &self.log_receiver {
                    for log in receiver.try_iter() {
                        self.logs.push_back(log);
                        if self.logs.len() > 100 {
                            self.logs.pop_front();
                        }
                    }
                }

                eframe::egui::ScrollArea::vertical().show(ui, |ui| {
                    for log in &self.logs {
                        ui.label(log);
                        ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                    }
                });

                if let Some(ref mut connection_info) = self.connection_info {
                    ui.label(format!("Verbonden met: {}", connection_info.port_path));
                    ui.label(format!("Baudrate: {}", connection_info.baudrate));
                }
            });


            // if self.show_connection_window {
            //     ctx.show_viewport_immediate(
            //         egui::ViewportId::from_hash_of("connection_window"),
            //         egui::ViewportBuilder::default()
            //             .with_title("Hardware Connection")
            //             .with_inner_size([300.0, 150.0]),
            //         |ctx, _class| {
            //             egui::CentralPanel::default().show(ctx, |ui| {
            //                 // ui.label(&self.connection_message);
    
            //                 if ui.button("Opnieuw proberen").clicked() {
            //                     if scan_and_connect().is_ok() {
            //                         self.show_connection_window = false;
            //                         self.is_connected = true;
            //                     }
            //                 }
    
            //                 if ui.button("Sluiten").clicked() {
            //                     self.show_connection_window = false
            //                 }
            //             });
            //         },
            //     );
            // }

            ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn scan_and_connect() -> Result<ConnectionInfo, String> {
    println!("scan and connect");
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
    println!("try connect");
    let port = serialport::new(port_name, 115200)
        .timeout(Duration::from_secs(5))
        .open();

    match port {
        Ok(mut port) => {
            // Bij succes, retourneer de ConnectionInfo
            let baudrate = 115200; // Dit kan worden ingesteld op basis van wat je nodig hebt
            Ok(ConnectionInfo::new(port_name.to_string(), baudrate, port))
        }
        Err(e) => Err(format!("Kon poort {} niet openen: {}", port_name, e)),
    }
}