//! Analyze workspace: breadcrumbs, tabs, filters, treemap, and tables.

use std::sync::Arc;

use diskclean_core::{NodeId, ROOT_ID, Tree};
use eframe::egui::{
    self, Align, Align2, Button, Color32, FontId, Layout, RichText, Sense, Stroke, StrokeKind,
    TextEdit, vec2,
};
use egui_extras::{Column, TableBuilder};

use crate::app::{
    DiskCleanupApp, SortKey, SortState, Tab, Workspace, kind_icon, kind_label, search_field_id,
};
use crate::i18n::Key;
use crate::ui::{components, format, icons, theme, tokens};

impl DiskCleanupApp {
    pub(crate) fn central(&mut self, ui: &mut egui::Ui) {
        match self.workspace {
            Workspace::Analyze => self.analyze_workspace(ui),
            Workspace::Cleanup => self.cleanup_workspace(ui),
            Workspace::Duplicates => self.duplicates_workspace(ui),
            Workspace::Applications => self.applications_workspace(ui),
        }
    }

    fn analyze_workspace(&mut self, ui: &mut egui::Ui) {
        let Some(tree) = self.tree.clone() else {
            self.overview(ui);
            return;
        };

        self.workspace_header(ui, &tree);
        self.workspace_tabs(ui);
        self.workspace_toolbar(ui);
        self.update_analysis(&tree);
        ui.add_space(8.0);

        match self.tab {
            Tab::Map => self.map_tab(ui, &tree),
            Tab::Files => self.files_tab(ui, &tree),
            Tab::Folders => self.folders_tab(ui, &tree),
            Tab::Types => self.types_tab(ui, &tree),
            Tab::Issues => self.issues_tab(ui, &tree),
        }
    }

    fn workspace_header(&mut self, ui: &mut egui::Ui, tree: &Arc<Tree>) {
        let palette = self.palette;
        let selected = self.selected;
        let mut chosen = None;
        let mut go_up = false;

        ui.horizontal(|ui| {
            if components::icon_button(ui, palette, icons::ARROW_UP, self.text(Key::Up)).clicked() {
                go_up = true;
            }

            let breadcrumb_width = (ui.available_width() - 150.0).max(140.0);
            ui.allocate_ui_with_layout(
                vec2(breadcrumb_width, 24.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("breadcrumbs")
                        .max_height(24.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let mut chain = Vec::new();
                                let mut current = Some(selected);
                                while let Some(id) = current {
                                    chain.push(id);
                                    current = tree.node(id).parent;
                                }
                                chain.reverse();

                                for (index, id) in chain.iter().enumerate() {
                                    if index > 0 {
                                        ui.label(
                                            RichText::new(icons::CARET_RIGHT)
                                                .color(palette.muted)
                                                .size(9.0),
                                        );
                                    }
                                    let label = if *id == ROOT_ID {
                                        tree.root_path.display().to_string()
                                    } else {
                                        tree.node(*id).display_name()
                                    };
                                    let response = ui
                                        .add(
                                            Button::new(
                                                RichText::new(format::shorten_middle(&label, 34))
                                                    .size(12.0)
                                                    .color(if selected == *id {
                                                        palette.text
                                                    } else {
                                                        palette.muted
                                                    }),
                                            )
                                            .frame(false)
                                            .fill(Color32::TRANSPARENT)
                                            .min_size(vec2(0.0, 22.0)),
                                        )
                                        .on_hover_text(label);
                                    if response.clicked() {
                                        chosen = Some(*id);
                                    }
                                }
                            });
                        });
                },
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let node = tree.node(selected);
                ui.label(
                    RichText::new(format::size(self.language(), node.allocated_size))
                        .font(FontId::monospace(11.0))
                        .color(palette.muted),
                );
            });
        });

        if go_up && let Some(parent) = tree.node(selected).parent {
            self.selected = parent;
        }
        if let Some(id) = chosen {
            self.selected = id;
        }
    }

    fn workspace_tabs(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let language = self.language();
        let mut chosen = None;

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let specs = [
                (Tab::Map, Key::Map, icons::TREE),
                (Tab::Files, Key::TopFiles, icons::FILE),
                (Tab::Folders, Key::TopFolders, icons::FOLDER),
                (Tab::Types, Key::Types, icons::LIST),
                (Tab::Issues, Key::Issues, icons::WARNING),
            ];
            for (tab, label, icon) in specs {
                let selected = self.tab == tab;
                let text = RichText::new(format!("{icon}  {}", language.text(label)))
                    .size(11.5)
                    .color(if selected {
                        palette.accent
                    } else {
                        palette.muted
                    });
                let response = ui.add(
                    Button::new(text)
                        .fill(if selected {
                            palette.accent_soft()
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(if selected {
                            Stroke::new(1.0, palette.accent)
                        } else {
                            Stroke::new(1.0, Color32::TRANSPARENT)
                        })
                        .corner_radius(12)
                        .min_size(vec2(0.0, 26.0)),
                );
                if response.clicked() {
                    chosen = Some(tab);
                }
            }
        });
        if let Some(tab) = chosen {
            self.tab = tab;
        }
    }

    fn workspace_toolbar(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        ui.add_space(5.0);
        ui.horizontal_wrapped(|ui| {
            let search_width = (ui.available_width() - 240.0).clamp(100.0, 340.0);
            let search_hint = self.text(Key::SearchHint);
            let response = ui.add(
                TextEdit::singleline(&mut self.filters.search)
                    .id(search_field_id())
                    .hint_text(search_hint)
                    .desired_width(search_width)
                    .margin(egui::Margin::symmetric(8, 5)),
            );
            if self.focus_search {
                response.request_focus();
                self.focus_search = false;
            }
            if !self.filters.search.is_empty()
                && components::icon_button(ui, palette, icons::CLOSE, self.text(Key::ClearSearch))
                    .clicked()
            {
                self.filters.search.clear();
            }

            if self.tab == Tab::Issues {
                ui.label(
                    RichText::new(self.text(Key::IssueSearchPaths))
                        .small()
                        .color(palette.muted),
                );
                return;
            }
            let active = self.filters_active();
            let label = if active > 0 {
                format!("{}  {} · {active}", icons::FILTER, self.text(Key::Filters))
            } else {
                format!("{}  {}", icons::FILTER, self.text(Key::Filters))
            };
            ui.menu_button(label, |ui| self.filter_popover(ui));

            if active > 0 {
                let response = ui.add(
                    Button::new(RichText::new(self.text(Key::ClearFilters)).size(10.5))
                        .frame(false)
                        .fill(Color32::TRANSPARENT),
                );
                if response.clicked() {
                    self.clear_filters();
                }
            }
        });

        if self.tab == Tab::Issues {
            return;
        }
        if self.filters_active() > 0 {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                let mut clear_search = false;
                if !self.filters.search.is_empty() {
                    let text = format!(
                        "“{}”  {}",
                        format::shorten_middle(self.filters.search.trim(), 28),
                        icons::CLOSE
                    );
                    if components::chip(ui, palette, &text, None).clicked() {
                        clear_search = true;
                    }
                }
                if self.min_size() > 0 {
                    let text = format!(
                        "≥ {}  {}",
                        format::size(self.language(), self.min_size()),
                        icons::CLOSE
                    );
                    if components::chip(ui, palette, &text, None).clicked() {
                        self.filters.min_size = "0".to_owned();
                    }
                }
                if !self.filters.show_files {
                    let text = format!("{}  {}", self.text(Key::File), icons::CLOSE);
                    if components::chip(ui, palette, &text, None).clicked() {
                        self.filters.show_files = true;
                    }
                }
                if !self.filters.show_dirs {
                    let text = format!("{}  {}", self.text(Key::Folder), icons::CLOSE);
                    if components::chip(ui, palette, &text, None).clicked() {
                        self.filters.show_dirs = true;
                    }
                }
                if clear_search {
                    self.filters.search.clear();
                }
            });
        }
    }

    fn filter_popover(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        ui.set_min_width(230.0);
        ui.label(RichText::new(self.text(Key::Filters)).font(theme::semibold(13.0)));
        ui.add_space(6.0);
        ui.label(
            RichText::new(self.text(Key::MinSize))
                .color(palette.muted)
                .size(11.0),
        );
        let min_size_hint = self.text(Key::MinSizeHint);
        ui.add(
            TextEdit::singleline(&mut self.filters.min_size)
                .hint_text(min_size_hint)
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(7, 5)),
        );
        if !self.filters.min_size.trim().is_empty()
            && diskclean_core::parse_size(self.filters.min_size.trim()).is_err()
        {
            ui.label(
                RichText::new(self.text(Key::InvalidSize))
                    .color(palette.danger)
                    .size(10.5),
            );
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(self.text(Key::Show))
                .color(palette.muted)
                .size(11.0),
        );
        let file_label = self.text(Key::File);
        let folder_label = self.text(Key::Folder);
        ui.checkbox(&mut self.filters.show_files, file_label);
        ui.checkbox(&mut self.filters.show_dirs, folder_label);
        ui.add_space(7.0);
        if ui.button(self.text(Key::Clear)).clicked() {
            self.clear_filters();
            ui.close();
        }
    }

    fn map_tab(&mut self, ui: &mut egui::Ui, tree: &Arc<Tree>) {
        let palette = self.palette;
        let selected_node = tree.node(self.selected);
        let map_root = if selected_node.kind.is_dir() {
            self.selected
        } else {
            selected_node.parent.unwrap_or(ROOT_ID)
        };

        let children = self.indexed_children(tree, map_root);
        let total_children = children.len();
        const MAX_TILES: usize = 500;
        let children = &children[..total_children.min(MAX_TILES)];

        if children.is_empty() {
            components::empty_hint(ui, palette, self.text(Key::NoMatches));
            return;
        }

        let items: Vec<(NodeId, u64)> = children
            .iter()
            .map(|id| {
                let node = tree.node(*id);
                (*id, node.allocated_size.max(node.logical_size))
            })
            .collect();

        let note_height = if total_children > MAX_TILES {
            22.0
        } else {
            0.0
        };
        let available = ui.available_size();
        let size = vec2(
            available.x.max(20.0),
            (available.y - note_height).max(120.0),
        );
        let (response, painter) = ui.allocate_painter(size, Sense::click());
        painter.rect_filled(response.rect, tokens::RADIUS_CARD, palette.surface);
        painter.rect_stroke(
            response.rect,
            tokens::RADIUS_CARD,
            Stroke::new(1.0, palette.border),
            StrokeKind::Inside,
        );
        let content_rect = response.rect.shrink(5.0);
        let tiles = crate::treemap::layout(&items, content_rect);
        let mut clicked = None;

        for (index, tile) in tiles.into_iter().enumerate() {
            let rect = tile.rect.shrink(1.0);
            if rect.width() < 2.0 || rect.height() < 2.0 {
                continue;
            }

            let node = tree.node(tile.id);
            let base = palette.tile_color(index);
            let color = match node.kind {
                diskclean_core::NodeKind::Directory => base,
                diskclean_core::NodeKind::Symlink => base.gamma_multiply(0.62),
                _ => base.gamma_multiply(0.84),
            };
            painter.rect_filled(rect, 3, color);

            if rect.width() > 46.0 && rect.height() > 17.0 {
                let clipped = painter.with_clip_rect(rect.shrink(2.0));
                let max_chars = ((rect.width() / 6.2) as usize).max(5);
                clipped.text(
                    rect.left_top() + vec2(5.0, 3.0),
                    Align2::LEFT_TOP,
                    format::shorten_middle(&node.display_name(), max_chars),
                    FontId::proportional(11.5),
                    palette.tile_text,
                );
                if rect.height() > 35.0 {
                    clipped.text(
                        rect.right_bottom() - vec2(5.0, 3.0),
                        Align2::RIGHT_BOTTOM,
                        format::size(self.language(), node.allocated_size),
                        FontId::monospace(10.0),
                        Color32::from_white_alpha(215),
                    );
                }
            }

            let response = ui.interact(
                rect,
                ui.scope_id().with(("treemap", map_root, tile.id)),
                Sense::click(),
            );
            if response.hovered() {
                painter.rect_filled(
                    rect,
                    3,
                    if palette.is_dark() {
                        Color32::from_white_alpha(16)
                    } else {
                        Color32::from_black_alpha(10)
                    },
                );
            }
            if self.selected == tile.id {
                painter.rect_stroke(rect, 3, Stroke::new(2.0, palette.text), StrokeKind::Inside);
            } else if response.hovered() {
                painter.rect_stroke(
                    rect,
                    3,
                    Stroke::new(1.0, palette.accent),
                    StrokeKind::Inside,
                );
            }
            if response.clicked() {
                clicked = Some(tile.id);
            }
            let tile_path = tree.path(tile.id);
            response.context_menu(|ui| self.path_context_menu(ui, &tile_path));
            response.on_hover_ui(|ui| {
                ui.label(RichText::new(node.display_name()).font(theme::semibold(13.0)));
                ui.label(tile_path.display().to_string());
                ui.label(format!(
                    "{}  ·  {}",
                    format::size(self.language(), node.allocated_size),
                    kind_label(self.language(), node.kind)
                ));
                ui.label(
                    RichText::new(if node.kind.is_dir() {
                        format!("{} {}", self.text(Key::Open), "↓")
                    } else {
                        self.text(Key::Selection).to_owned()
                    })
                    .color(palette.muted)
                    .size(10.5),
                );
            });
        }

        if let Some(id) = clicked {
            self.selected = id;
        }
        if total_children > MAX_TILES {
            ui.label(
                RichText::new(format!(
                    "{} {MAX_TILES} {} {total_children}",
                    self.text(Key::Showing),
                    self.text(Key::Of)
                ))
                .color(palette.muted)
                .size(10.5),
            );
        }
    }

    fn files_tab(&mut self, ui: &mut egui::Ui, tree: &Arc<Tree>) {
        if !self.filters.show_files {
            components::empty_hint(ui, self.palette, self.text(Key::NoFiles));
            return;
        }

        let files = Arc::clone(&self.analysis_view.files);
        let index = Arc::clone(&self.analysis_index);

        if files.is_empty() {
            components::empty_hint(ui, self.palette, self.text(Key::NoFiles));
            return;
        }

        let palette = self.palette;
        let language = self.language();
        let selected = self.selected;
        let sort = self.file_sort;
        let labels = [
            self.text(Key::Name),
            self.text(Key::Size),
            self.text(Key::Modified),
            self.text(Key::Kind),
        ];
        let mut chosen = None;
        let mut next_sort = None;

        egui::ScrollArea::horizontal()
            .id_salt("top_files_table_horizontal")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(tokens::WORKSPACE_MIN_WIDTH);
                TableBuilder::new(ui)
                    .id_salt("top_files_table")
                    .striped(false)
                    .resizable(true)
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .column(Column::remainder().clip(true).at_least(150.0))
                    .column(Column::exact(92.0).clip(true))
                    .column(Column::exact(96.0).clip(true))
                    .column(Column::exact(74.0).clip(true))
                    .header(tokens::HEADER_HEIGHT, |mut header| {
                        header.col(|ui| {
                            if components::sort_header(
                                ui,
                                palette,
                                labels[0],
                                sort.key == SortKey::Name,
                                sort.descending,
                            )
                            .clicked()
                            {
                                next_sort = Some(next_sort_state(sort, SortKey::Name));
                            }
                        });
                        header.col(|ui| {
                            if components::sort_header(
                                ui,
                                palette,
                                labels[1],
                                sort.key == SortKey::Size,
                                sort.descending,
                            )
                            .clicked()
                            {
                                next_sort = Some(next_sort_state(sort, SortKey::Size));
                            }
                        });
                        header.col(|ui| {
                            if components::sort_header(
                                ui,
                                palette,
                                labels[2],
                                sort.key == SortKey::Modified,
                                sort.descending,
                            )
                            .clicked()
                            {
                                next_sort = Some(next_sort_state(sort, SortKey::Modified));
                            }
                        });
                        header.col(|ui| {
                            ui.label(RichText::new(labels[3]).color(palette.muted).size(10.5));
                        });
                    })
                    .body(|body| {
                        body.rows(tokens::ROW_HEIGHT, files.len(), |mut row| {
                            let id = files[row.index()];
                            let node = tree.node(id);
                            let path = index.paths[id as usize].clone();
                            row.set_selected(selected == id);
                            row.col(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(kind_icon(node.kind))
                                            .color(palette.muted)
                                            .size(12.0),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(node.display_name()).size(12.0),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(path.display().to_string());
                                    if node.is_hardlink_duplicate {
                                        ui.label(
                                            RichText::new(icons::DUPLICATES)
                                                .color(palette.warning)
                                                .size(11.0),
                                        )
                                        .on_hover_text(self.text(Key::Hardlink));
                                    }
                                });
                            });
                            row.col(|ui| {
                                components::right_label(
                                    ui,
                                    palette,
                                    &format::size(language, node.allocated_size),
                                    true,
                                );
                            });
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(format::age(language, node.modified_unix))
                                        .color(palette.muted)
                                        .size(10.5),
                                );
                            });
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(kind_label(language, node.kind))
                                        .color(palette.muted)
                                        .size(10.5),
                                );
                            });
                            let response = row.response();
                            if response.clicked() {
                                chosen = Some(id);
                            }
                            response.context_menu(|ui| self.path_context_menu(ui, &path));
                            response.on_hover_text(path.display().to_string());
                        });
                    });
            });

        if let Some(sort) = next_sort {
            self.file_sort = sort;
        }
        if let Some(id) = chosen {
            self.selected = id;
        }
    }

    fn path_context_menu(&mut self, ui: &mut egui::Ui, path: &std::path::Path) {
        if ui.button(self.text(Key::Open)).clicked() {
            self.open_path(path);
            ui.close();
        }
        if ui.button(self.text(Key::Reveal)).clicked() {
            self.reveal_path(path);
            ui.close();
        }
        if ui.button(self.text(Key::CopyPath)).clicked() {
            self.copy_path(ui.ctx(), path);
            ui.close();
        }
    }

    fn folders_tab(&mut self, ui: &mut egui::Ui, tree: &Arc<Tree>) {
        if !self.filters.show_dirs {
            components::empty_hint(ui, self.palette, self.text(Key::NoFolders));
            return;
        }

        let folders = Arc::clone(&self.analysis_view.folders);
        let index = Arc::clone(&self.analysis_index);

        if folders.is_empty() {
            components::empty_hint(ui, self.palette, self.text(Key::NoFolders));
            return;
        }

        let palette = self.palette;
        let language = self.language();
        let selected = self.selected;
        let sort = self.folder_sort;
        let labels = [
            self.text(Key::Name),
            self.text(Key::Size),
            self.text(Key::Files),
            self.text(Key::Folders),
        ];
        let mut chosen = None;
        let mut next_sort = None;

        egui::ScrollArea::horizontal()
            .id_salt("top_folders_table_horizontal")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(tokens::WORKSPACE_MIN_WIDTH);
                TableBuilder::new(ui)
                    .id_salt("top_folders_table")
                    .striped(false)
                    .resizable(true)
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .column(Column::remainder().clip(true).at_least(150.0))
                    .column(Column::exact(92.0).clip(true))
                    .column(Column::exact(78.0).clip(true))
                    .column(Column::exact(78.0).clip(true))
                    .header(tokens::HEADER_HEIGHT, |mut header| {
                        header.col(|ui| {
                            if components::sort_header(
                                ui,
                                palette,
                                labels[0],
                                sort.key == SortKey::Name,
                                sort.descending,
                            )
                            .clicked()
                            {
                                next_sort = Some(next_sort_state(sort, SortKey::Name));
                            }
                        });
                        header.col(|ui| {
                            if components::sort_header(
                                ui,
                                palette,
                                labels[1],
                                sort.key == SortKey::Size,
                                sort.descending,
                            )
                            .clicked()
                            {
                                next_sort = Some(next_sort_state(sort, SortKey::Size));
                            }
                        });
                        header.col(|ui| {
                            ui.label(RichText::new(labels[2]).color(palette.muted).size(10.5));
                        });
                        header.col(|ui| {
                            ui.label(RichText::new(labels[3]).color(palette.muted).size(10.5));
                        });
                    })
                    .body(|body| {
                        body.rows(tokens::ROW_HEIGHT, folders.len(), |mut row| {
                            let id = folders[row.index()];
                            let node = tree.node(id);
                            let path = index.paths[id as usize].clone();
                            row.set_selected(selected == id);
                            row.col(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(icons::FOLDER)
                                            .color(palette.accent)
                                            .size(12.0),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(node.display_name()).size(12.0),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(path.display().to_string());
                                });
                            });
                            row.col(|ui| {
                                components::right_label(
                                    ui,
                                    palette,
                                    &format::size(language, node.allocated_size),
                                    true,
                                );
                            });
                            row.col(|ui| {
                                components::right_label(
                                    ui,
                                    palette,
                                    &node.file_count.to_string(),
                                    false,
                                );
                            });
                            row.col(|ui| {
                                components::right_label(
                                    ui,
                                    palette,
                                    &node.dir_count.saturating_sub(1).to_string(),
                                    false,
                                );
                            });
                            let response = row.response();
                            if response.clicked() {
                                chosen = Some(id);
                            }
                            response.context_menu(|ui| self.path_context_menu(ui, &path));
                            response.on_hover_text(path.display().to_string());
                        });
                    });
            });

        if let Some(sort) = next_sort {
            self.folder_sort = sort;
        }
        if let Some(id) = chosen {
            self.selected = id;
        }
    }

    fn types_tab(&mut self, ui: &mut egui::Ui, _tree: &Arc<Tree>) {
        let palette = self.palette;
        let language = self.language();
        let extensions = Arc::clone(&self.analysis_view.extensions);

        if extensions.is_empty() {
            components::empty_hint(ui, palette, self.text(Key::NoTypes));
            return;
        }

        let total_size = extensions
            .iter()
            .fold(0_u64, |sum, extension| {
                sum.saturating_add(extension.allocated_bytes)
            })
            .max(1);
        let sort = self.type_sort;
        let mut next_sort = None;
        let labels = [
            self.text(Key::Types),
            self.text(Key::Files),
            self.text(Key::Size),
            self.text(Key::Share),
        ];

        egui::ScrollArea::horizontal()
            .id_salt("extensions_table_horizontal")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(tokens::WORKSPACE_MIN_WIDTH);
                TableBuilder::new(ui)
                    .id_salt("extensions_table")
                    .striped(false)
                    .resizable(true)
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .column(Column::remainder().clip(true).at_least(120.0))
                    .column(Column::exact(72.0).clip(true))
                    .column(Column::exact(92.0).clip(true))
                    .column(Column::exact(150.0))
                    .header(tokens::HEADER_HEIGHT, |mut header| {
                        for (column, label) in labels.into_iter().enumerate() {
                            header.col(|ui| {
                                let key = match column {
                                    0 => SortKey::Name,
                                    1 => SortKey::Count,
                                    _ => SortKey::Size,
                                };
                                if components::sort_header(
                                    ui,
                                    palette,
                                    label,
                                    sort.key == key,
                                    sort.descending,
                                )
                                .clicked()
                                {
                                    next_sort = Some(next_sort_state(sort, key));
                                }
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(tokens::ROW_HEIGHT, extensions.len(), |mut row| {
                            let extension = &extensions[row.index()];
                            row.col(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(icons::FILE).color(palette.muted).size(12.0),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(format!(".{}", extension.extension))
                                                .font(theme::medium(12.0)),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(&extension.extension);
                                });
                            });
                            row.col(|ui| {
                                components::right_label(
                                    ui,
                                    palette,
                                    &extension.files.to_string(),
                                    false,
                                );
                            });
                            row.col(|ui| {
                                components::right_label(
                                    ui,
                                    palette,
                                    &format::size(language, extension.allocated_bytes),
                                    true,
                                );
                            });
                            row.col(|ui| {
                                let fraction = (extension.allocated_bytes as f32
                                    / total_size as f32)
                                    .clamp(0.0, 1.0);
                                ui.add(
                                    egui::ProgressBar::new(fraction)
                                        .desired_height(5.0)
                                        .fill(palette.accent),
                                );
                            });
                        });
                    });
            });
        if let Some(sort) = next_sort {
            self.type_sort = sort;
        }
    }

    fn issues_tab(&mut self, ui: &mut egui::Ui, tree: &Arc<Tree>) {
        let palette = self.palette;
        let language = self.language();
        egui::ComboBox::from_id_salt("issue_kind")
            .selected_text(
                self.issue_kind
                    .map_or(self.text(Key::All), |kind| issue_label(language, kind)),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.issue_kind, None, language.text(Key::All));
                for kind in [
                    diskclean_core::IssueKind::PermissionDenied,
                    diskclean_core::IssueKind::NotFound,
                    diskclean_core::IssueKind::ReadDir,
                    diskclean_core::IssueKind::Metadata,
                    diskclean_core::IssueKind::Other,
                ] {
                    ui.selectable_value(
                        &mut self.issue_kind,
                        Some(kind),
                        issue_label(language, kind),
                    );
                }
            });
        self.update_analysis(tree);
        let issues = Arc::clone(&self.analysis_view.issues);
        if issues.is_empty() {
            components::empty_hint(ui, palette, self.text(Key::NoIssues));
            return;
        }

        let labels = [
            self.text(Key::Kind),
            self.text(Key::Path),
            self.text(Key::Message),
        ];
        egui::ScrollArea::horizontal()
            .id_salt("issues_table_horizontal")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(tokens::WORKSPACE_MIN_WIDTH);
                TableBuilder::new(ui)
                    .id_salt("issues_table")
                    .striped(false)
                    .resizable(true)
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .column(Column::exact(120.0).clip(true))
                    .column(Column::remainder().clip(true).at_least(170.0))
                    .column(Column::remainder().clip(true).at_least(200.0))
                    .header(tokens::HEADER_HEIGHT, |mut header| {
                        for label in labels {
                            header.col(|ui| {
                                ui.label(RichText::new(label).color(palette.muted).size(10.5));
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(tokens::ROW_HEIGHT, issues.len(), |mut row| {
                            let issue = &tree.issues[issues[row.index()]];
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(issue_label(language, issue.kind))
                                        .color(palette.warning)
                                        .size(10.5),
                                );
                            });
                            row.col(|ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(issue.path.display().to_string()).size(11.0),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(issue.path.display().to_string());
                            });
                            row.col(|ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(issue_summary(language, issue.kind))
                                            .size(11.0),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&issue.message);
                            });
                            row.response().context_menu(|ui| {
                                if ui.button(language.text(Key::CopyPath)).clicked() {
                                    ui.ctx().copy_text(issue.path.display().to_string());
                                    ui.close();
                                }
                            });
                        });
                    });
            });
    }
}

fn issue_label(language: crate::i18n::Language, kind: diskclean_core::IssueKind) -> &'static str {
    language.text(match kind {
        diskclean_core::IssueKind::PermissionDenied => Key::IssuePermission,
        diskclean_core::IssueKind::NotFound => Key::IssueMissing,
        diskclean_core::IssueKind::ReadDir => Key::IssueReadDirectory,
        diskclean_core::IssueKind::Metadata => Key::IssueMetadata,
        diskclean_core::IssueKind::Other => Key::Other,
    })
}

fn issue_summary(language: crate::i18n::Language, kind: diskclean_core::IssueKind) -> &'static str {
    language.text(match kind {
        diskclean_core::IssueKind::PermissionDenied => Key::IssuePermissionBody,
        diskclean_core::IssueKind::NotFound => Key::IssueMissingBody,
        _ => Key::IssueReadBody,
    })
}

fn next_sort_state(current: SortState, key: SortKey) -> SortState {
    if current.key == key {
        SortState {
            key,
            descending: !current.descending,
        }
    } else {
        SortState {
            key,
            descending: key == SortKey::Size,
        }
    }
}
