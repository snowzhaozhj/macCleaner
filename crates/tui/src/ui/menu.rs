use crate::app::App;
use crate::theme;
use crate::ui::chrome;
use ratatui::layout::{Constraint, Direction, Layout, Alignment};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

const MENU_ITEMS: &[(&str, &str)] = &[
    ("Clean", "扫描并清理系统缓存、日志、临时文件"),
    ("Uninstall", "卸载应用及其残留文件"),
    ("Analyze", "磁盘空间分析器 — 交互式浏览"),
    ("Purge", "扫描并清理开发产物 (node_modules, target 等)"),
];

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(f.area());

    // 标题
    let title = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " macCleaner ",
                Style::default()
                    .fg(theme::accent())
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            " macOS 系统清理工具",
            Style::default().fg(theme::ink_muted()),
        )),
    ])
    .alignment(Alignment::Center);
    f.render_widget(title, chunks[0]);

    // 菜单列表
    let items: Vec<ListItem> = MENU_ITEMS
        .iter()
        .enumerate()
        .map(|(i, (name, desc))| {
            let marker = if i == app.menu_index { "▶ " } else { "  " };
            let style = if i == app.menu_index {
                Style::default()
                    .fg(theme::accent())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::ink())
            };
            ListItem::new(vec![
                Line::from(Span::styled(format!("{marker}{name}"), style)),
                Line::from(Span::styled(
                    format!("    {desc}"),
                    Style::default().fg(theme::ink_muted()),
                )),
            ])
        })
        .collect();

    let menu = List::new(items)
        .block(
            Block::default()
                .title(" 选择操作 ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::accent())),
        );

    let mut state = ListState::default();
    state.select(Some(app.menu_index));
    f.render_stateful_widget(menu, chunks[1], &mut state);

    // 底部提示
    chrome::render_footer(f, chunks[2], &crate::keymap::footer_line(&app.state, chunks[2].width as usize));
}
