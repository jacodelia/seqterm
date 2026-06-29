//! Lightweight internationalisation.
//!
//! Design: the English source text **is** the lookup key. `t("Save")` returns the
//! translation for the current language, or the key itself when the language is
//! English or no translation exists. New strings become translatable simply by
//! wrapping them in `t(..)`; until a table entry is added they fall back to English.
//!
//! The current language is a process-global atomic so render code can call `t`
//! without threading state through every function.

use std::sync::atomic::{AtomicU8, Ordering};

use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    En,
    Es,
    Ru,
    Ja,
    Zh,
    Fr,
    It,
    De,
}

impl Language {
    pub const ALL: &'static [Language] = &[
        Language::En, Language::Es, Language::Ru, Language::Ja,
        Language::Zh, Language::Fr, Language::It, Language::De,
    ];

    /// Stable short code persisted in settings.
    pub fn code(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Es => "es",
            Language::Ru => "ru",
            Language::Ja => "ja",
            Language::Zh => "zh",
            Language::Fr => "fr",
            Language::It => "it",
            Language::De => "de",
        }
    }

    /// Name shown in the language picker — always in its own language.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::En => "English",
            Language::Es => "Español",
            Language::Ru => "Русский",
            Language::Ja => "日本語",
            Language::Zh => "中文",
            Language::Fr => "Français",
            Language::It => "Italiano",
            Language::De => "Deutsch",
        }
    }

    pub fn from_code(code: &str) -> Language {
        Language::ALL.iter().copied()
            .find(|l| l.code() == code)
            .unwrap_or(Language::En)
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0); // 0 == En

pub fn set_language(lang: Language) {
    let idx = Language::ALL.iter().position(|&l| l == lang).unwrap_or(0);
    CURRENT.store(idx as u8, Ordering::Relaxed);
}

pub fn current() -> Language {
    let idx = CURRENT.load(Ordering::Relaxed) as usize;
    Language::ALL.get(idx).copied().unwrap_or(Language::En)
}

/// Translate `key` to the current language, or return `key` unchanged when the
/// language is English or the key has no entry.
pub fn t(key: &'static str) -> &'static str {
    let table = match current() {
        Language::En => return key,
        Language::Es => ES,
        Language::Ru => RU,
        Language::Ja => JA,
        Language::Zh => ZH,
        Language::Fr => FR,
        Language::It => IT,
        Language::De => DE,
    };
    // ponytail: linear scan over a small table. Swap to a HashMap if the chrome
    // string set grows past a few hundred entries.
    table.iter()
        .find_map(|&(k, v)| (k == key).then_some(v))
        .unwrap_or(key)
}

/// Terminal display width of `s` (handles CJK double-width + combining marks).
pub fn disp_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

type Table = &'static [(&'static str, &'static str)];

// Keys are the exact English source strings. Order is irrelevant.

static ES: Table = &[
    ("FILE", "ARCHIVO"), ("EDIT", "EDITAR"), ("HELP", "AYUDA"),
    ("New Project", "Nuevo proyecto"), ("Open Project…", "Abrir proyecto…"),
    ("Save", "Guardar"), ("Save As…", "Guardar como…"),
    ("Import MIDI…", "Importar MIDI…"), ("Export MIDI…", "Exportar MIDI…"),
    ("Export MusicXML…", "Exportar MusicXML…"), ("Export Audio…", "Exportar audio…"),
    ("Exit", "Salir"),
    ("Undo", "Deshacer"), ("Redo", "Rehacer"),
    ("Routing Config", "Configuración de enrutamiento"), ("Settings…", "Ajustes…"),
    ("Keyboard Shortcuts", "Atajos de teclado"), ("Workflow Guide", "Guía de flujo de trabajo"),
    ("MIDI Import Guide", "Guía de importación MIDI"), ("Routing Guide", "Guía de enrutamiento"),
    ("Pattern Editor Guide", "Guía del editor de patrones"), ("Troubleshooting", "Solución de problemas"),
    ("Latency Optimization", "Optimización de latencia"), ("About SeqTerm", "Acerca de SeqTerm"),
    ("Cancel", "Cancelar"), ("Apply", "Aplicar"), ("Export", "Exportar"),
    ("Select", "Seleccionar"), ("Confirm", "Confirmar"), ("Assign", "Asignar"),
    ("Import", "Importar"), ("Open", "Abrir"),
    ("Settings", "Ajustes"), ("Audio", "Audio"), ("Keybindings", "Atajos de teclado"),
    ("Language", "Idioma"), ("Audio Settings", "Ajustes de audio"), ("MIDI Settings", "Ajustes MIDI"),
    ("↑↓ navigate · Enter select · Esc close", "↑↓ navegar · Enter seleccionar · Esc cerrar"),
    ("Select language", "Seleccionar idioma"),
];

static RU: Table = &[
    ("FILE", "ФАЙЛ"), ("EDIT", "ПРАВКА"), ("HELP", "СПРАВКА"),
    ("New Project", "Новый проект"), ("Open Project…", "Открыть проект…"),
    ("Save", "Сохранить"), ("Save As…", "Сохранить как…"),
    ("Import MIDI…", "Импорт MIDI…"), ("Export MIDI…", "Экспорт MIDI…"),
    ("Export MusicXML…", "Экспорт MusicXML…"), ("Export Audio…", "Экспорт аудио…"),
    ("Exit", "Выход"),
    ("Undo", "Отменить"), ("Redo", "Повторить"),
    ("Routing Config", "Настройка маршрутизации"), ("Settings…", "Настройки…"),
    ("Keyboard Shortcuts", "Горячие клавиши"), ("Workflow Guide", "Руководство по работе"),
    ("MIDI Import Guide", "Руководство по импорту MIDI"), ("Routing Guide", "Руководство по маршрутизации"),
    ("Pattern Editor Guide", "Руководство по редактору паттернов"), ("Troubleshooting", "Устранение неполадок"),
    ("Latency Optimization", "Оптимизация задержки"), ("About SeqTerm", "О программе SeqTerm"),
    ("Cancel", "Отмена"), ("Apply", "Применить"), ("Export", "Экспорт"),
    ("Select", "Выбрать"), ("Confirm", "Подтвердить"), ("Assign", "Назначить"),
    ("Import", "Импорт"), ("Open", "Открыть"),
    ("Settings", "Настройки"), ("Audio", "Аудио"), ("Keybindings", "Горячие клавиши"),
    ("Language", "Язык"), ("Audio Settings", "Настройки аудио"), ("MIDI Settings", "Настройки MIDI"),
    ("↑↓ navigate · Enter select · Esc close", "↑↓ навигация · Enter выбрать · Esc закрыть"),
    ("Select language", "Выбор языка"),
];

static JA: Table = &[
    ("FILE", "ファイル"), ("EDIT", "編集"), ("HELP", "ヘルプ"),
    ("New Project", "新規プロジェクト"), ("Open Project…", "プロジェクトを開く…"),
    ("Save", "保存"), ("Save As…", "名前を付けて保存…"),
    ("Import MIDI…", "MIDIをインポート…"), ("Export MIDI…", "MIDIをエクスポート…"),
    ("Export MusicXML…", "MusicXMLをエクスポート…"), ("Export Audio…", "オーディオをエクスポート…"),
    ("Exit", "終了"),
    ("Undo", "元に戻す"), ("Redo", "やり直し"),
    ("Routing Config", "ルーティング設定"), ("Settings…", "設定…"),
    ("Keyboard Shortcuts", "キーボードショートカット"), ("Workflow Guide", "ワークフローガイド"),
    ("MIDI Import Guide", "MIDIインポートガイド"), ("Routing Guide", "ルーティングガイド"),
    ("Pattern Editor Guide", "パターンエディタガイド"), ("Troubleshooting", "トラブルシューティング"),
    ("Latency Optimization", "レイテンシ最適化"), ("About SeqTerm", "SeqTermについて"),
    ("Cancel", "キャンセル"), ("Apply", "適用"), ("Export", "エクスポート"),
    ("Select", "選択"), ("Confirm", "確認"), ("Assign", "割り当て"),
    ("Import", "インポート"), ("Open", "開く"),
    ("Settings", "設定"), ("Audio", "オーディオ"), ("Keybindings", "キー割り当て"),
    ("Language", "言語"), ("Audio Settings", "オーディオ設定"), ("MIDI Settings", "MIDI設定"),
    ("↑↓ navigate · Enter select · Esc close", "↑↓ 移動 · Enter 選択 · Esc 閉じる"),
    ("Select language", "言語を選択"),
];

static ZH: Table = &[
    ("FILE", "文件"), ("EDIT", "编辑"), ("HELP", "帮助"),
    ("New Project", "新建项目"), ("Open Project…", "打开项目…"),
    ("Save", "保存"), ("Save As…", "另存为…"),
    ("Import MIDI…", "导入 MIDI…"), ("Export MIDI…", "导出 MIDI…"),
    ("Export MusicXML…", "导出 MusicXML…"), ("Export Audio…", "导出音频…"),
    ("Exit", "退出"),
    ("Undo", "撤销"), ("Redo", "重做"),
    ("Routing Config", "路由配置"), ("Settings…", "设置…"),
    ("Keyboard Shortcuts", "键盘快捷键"), ("Workflow Guide", "工作流程指南"),
    ("MIDI Import Guide", "MIDI 导入指南"), ("Routing Guide", "路由指南"),
    ("Pattern Editor Guide", "模式编辑器指南"), ("Troubleshooting", "故障排除"),
    ("Latency Optimization", "延迟优化"), ("About SeqTerm", "关于 SeqTerm"),
    ("Cancel", "取消"), ("Apply", "应用"), ("Export", "导出"),
    ("Select", "选择"), ("Confirm", "确认"), ("Assign", "分配"),
    ("Import", "导入"), ("Open", "打开"),
    ("Settings", "设置"), ("Audio", "音频"), ("Keybindings", "键位绑定"),
    ("Language", "语言"), ("Audio Settings", "音频设置"), ("MIDI Settings", "MIDI 设置"),
    ("↑↓ navigate · Enter select · Esc close", "↑↓ 导航 · Enter 选择 · Esc 关闭"),
    ("Select language", "选择语言"),
];

static FR: Table = &[
    ("FILE", "FICHIER"), ("EDIT", "ÉDITION"), ("HELP", "AIDE"),
    ("New Project", "Nouveau projet"), ("Open Project…", "Ouvrir un projet…"),
    ("Save", "Enregistrer"), ("Save As…", "Enregistrer sous…"),
    ("Import MIDI…", "Importer MIDI…"), ("Export MIDI…", "Exporter MIDI…"),
    ("Export MusicXML…", "Exporter MusicXML…"), ("Export Audio…", "Exporter l'audio…"),
    ("Exit", "Quitter"),
    ("Undo", "Annuler"), ("Redo", "Rétablir"),
    ("Routing Config", "Configuration du routage"), ("Settings…", "Paramètres…"),
    ("Keyboard Shortcuts", "Raccourcis clavier"), ("Workflow Guide", "Guide du flux de travail"),
    ("MIDI Import Guide", "Guide d'import MIDI"), ("Routing Guide", "Guide de routage"),
    ("Pattern Editor Guide", "Guide de l'éditeur de motifs"), ("Troubleshooting", "Dépannage"),
    ("Latency Optimization", "Optimisation de la latence"), ("About SeqTerm", "À propos de SeqTerm"),
    ("Cancel", "Annuler"), ("Apply", "Appliquer"), ("Export", "Exporter"),
    ("Select", "Sélectionner"), ("Confirm", "Confirmer"), ("Assign", "Assigner"),
    ("Import", "Importer"), ("Open", "Ouvrir"),
    ("Settings", "Paramètres"), ("Audio", "Audio"), ("Keybindings", "Raccourcis"),
    ("Language", "Langue"), ("Audio Settings", "Paramètres audio"), ("MIDI Settings", "Paramètres MIDI"),
    ("↑↓ navigate · Enter select · Esc close", "↑↓ naviguer · Entrée sélectionner · Échap fermer"),
    ("Select language", "Choisir la langue"),
];

static IT: Table = &[
    ("FILE", "FILE"), ("EDIT", "MODIFICA"), ("HELP", "AIUTO"),
    ("New Project", "Nuovo progetto"), ("Open Project…", "Apri progetto…"),
    ("Save", "Salva"), ("Save As…", "Salva come…"),
    ("Import MIDI…", "Importa MIDI…"), ("Export MIDI…", "Esporta MIDI…"),
    ("Export MusicXML…", "Esporta MusicXML…"), ("Export Audio…", "Esporta audio…"),
    ("Exit", "Esci"),
    ("Undo", "Annulla"), ("Redo", "Ripeti"),
    ("Routing Config", "Configurazione routing"), ("Settings…", "Impostazioni…"),
    ("Keyboard Shortcuts", "Scorciatoie da tastiera"), ("Workflow Guide", "Guida al flusso di lavoro"),
    ("MIDI Import Guide", "Guida importazione MIDI"), ("Routing Guide", "Guida al routing"),
    ("Pattern Editor Guide", "Guida editor pattern"), ("Troubleshooting", "Risoluzione problemi"),
    ("Latency Optimization", "Ottimizzazione latenza"), ("About SeqTerm", "Informazioni su SeqTerm"),
    ("Cancel", "Annulla"), ("Apply", "Applica"), ("Export", "Esporta"),
    ("Select", "Seleziona"), ("Confirm", "Conferma"), ("Assign", "Assegna"),
    ("Import", "Importa"), ("Open", "Apri"),
    ("Settings", "Impostazioni"), ("Audio", "Audio"), ("Keybindings", "Tasti"),
    ("Language", "Lingua"), ("Audio Settings", "Impostazioni audio"), ("MIDI Settings", "Impostazioni MIDI"),
    ("↑↓ navigate · Enter select · Esc close", "↑↓ naviga · Invio seleziona · Esc chiudi"),
    ("Select language", "Seleziona lingua"),
];

static DE: Table = &[
    ("FILE", "DATEI"), ("EDIT", "BEARBEITEN"), ("HELP", "HILFE"),
    ("New Project", "Neues Projekt"), ("Open Project…", "Projekt öffnen…"),
    ("Save", "Speichern"), ("Save As…", "Speichern unter…"),
    ("Import MIDI…", "MIDI importieren…"), ("Export MIDI…", "MIDI exportieren…"),
    ("Export MusicXML…", "MusicXML exportieren…"), ("Export Audio…", "Audio exportieren…"),
    ("Exit", "Beenden"),
    ("Undo", "Rückgängig"), ("Redo", "Wiederholen"),
    ("Routing Config", "Routing-Konfiguration"), ("Settings…", "Einstellungen…"),
    ("Keyboard Shortcuts", "Tastenkürzel"), ("Workflow Guide", "Workflow-Anleitung"),
    ("MIDI Import Guide", "MIDI-Import-Anleitung"), ("Routing Guide", "Routing-Anleitung"),
    ("Pattern Editor Guide", "Pattern-Editor-Anleitung"), ("Troubleshooting", "Fehlerbehebung"),
    ("Latency Optimization", "Latenz-Optimierung"), ("About SeqTerm", "Über SeqTerm"),
    ("Cancel", "Abbrechen"), ("Apply", "Anwenden"), ("Export", "Exportieren"),
    ("Select", "Auswählen"), ("Confirm", "Bestätigen"), ("Assign", "Zuweisen"),
    ("Import", "Importieren"), ("Open", "Öffnen"),
    ("Settings", "Einstellungen"), ("Audio", "Audio"), ("Keybindings", "Tastenbelegung"),
    ("Language", "Sprache"), ("Audio Settings", "Audio-Einstellungen"), ("MIDI Settings", "MIDI-Einstellungen"),
    ("↑↓ navigate · Enter select · Esc close", "↑↓ navigieren · Enter wählen · Esc schließen"),
    ("Select language", "Sprache wählen"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_and_translate() {
        set_language(Language::En);
        assert_eq!(t("Save"), "Save");
        set_language(Language::Es);
        assert_eq!(t("Save"), "Guardar");
        assert_eq!(t("OK"), "OK"); // no entry → English fallback
        set_language(Language::En); // reset for other tests
    }

    #[test]
    fn code_roundtrip() {
        for &l in Language::ALL {
            assert_eq!(Language::from_code(l.code()), l);
        }
    }
}
