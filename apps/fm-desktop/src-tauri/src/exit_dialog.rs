//! Localised text of the native "background work is running" quit confirmation.
//!
//! The dialog is raised by the Rust shell, outside the web view, so it cannot use the frontend
//! catalogues; it follows the language stored in the settings instead.

use fm_transport_dto::LanguageDto;

/// Everything the quit confirmation shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExitDialogText {
    /// Dialog title.
    pub title: &'static str,
    /// Body when file operations and semantic indexing are running.
    pub operations_and_indexing: &'static str,
    /// Body when only file operations are running.
    pub operations: &'static str,
    /// Body when only semantic indexing is running.
    pub indexing: &'static str,
    /// Label of the button that quits anyway.
    pub quit_anyway: &'static str,
    /// Label of the button that keeps the app open.
    pub keep_working: &'static str,
}

const EN: ExitDialogText = ExitDialogText {
    title: "Background work is running",
    operations_and_indexing: "File operations will stop and will not resume automatically. Partial deletes or moves remain changed. Semantic indexing will reconcile on restart.",
    operations: "File operations will stop and will not resume automatically. Partial deletes or moves remain changed.",
    indexing: "Semantic indexing will stop and reconcile enrolled folders on restart.",
    quit_anyway: "Quit anyway",
    keep_working: "Keep working",
};

const NL: ExitDialogText = ExitDialogText {
    title: "Er draait werk op de achtergrond",
    operations_and_indexing: "Bestandsbewerkingen worden gestopt en niet automatisch hervat. Gedeeltelijke verwijderingen of verplaatsingen blijven bestaan. Semantisch indexeren wordt bij het herstarten bijgewerkt.",
    operations: "Bestandsbewerkingen worden gestopt en niet automatisch hervat. Gedeeltelijke verwijderingen of verplaatsingen blijven bestaan.",
    indexing: "Semantisch indexeren wordt gestopt en de opgenomen mappen worden bij het herstarten bijgewerkt.",
    quit_anyway: "Toch afsluiten",
    keep_working: "Doorgaan met werken",
};

const DE: ExitDialogText = ExitDialogText {
    title: "Hintergrundarbeit läuft",
    operations_and_indexing: "Dateivorgänge werden beendet und nicht automatisch fortgesetzt. Teilweise gelöschte oder verschobene Dateien bleiben verändert. Die semantische Indexierung wird beim Neustart abgeglichen.",
    operations: "Dateivorgänge werden beendet und nicht automatisch fortgesetzt. Teilweise gelöschte oder verschobene Dateien bleiben verändert.",
    indexing: "Die semantische Indexierung wird beendet und gleicht die aufgenommenen Ordner beim Neustart ab.",
    quit_anyway: "Trotzdem beenden",
    keep_working: "Weiterarbeiten",
};

const FR: ExitDialogText = ExitDialogText {
    title: "Des tâches s'exécutent en arrière-plan",
    operations_and_indexing: "Les opérations sur les fichiers s'arrêteront et ne reprendront pas automatiquement. Les suppressions ou déplacements partiels restent effectués. L'indexation sémantique sera rapprochée au redémarrage.",
    operations: "Les opérations sur les fichiers s'arrêteront et ne reprendront pas automatiquement. Les suppressions ou déplacements partiels restent effectués.",
    indexing: "L'indexation sémantique s'arrêtera et rapprochera les dossiers inclus au redémarrage.",
    quit_anyway: "Quitter quand même",
    keep_working: "Continuer à travailler",
};

const ES: ExitDialogText = ExitDialogText {
    title: "Hay trabajo en segundo plano",
    operations_and_indexing: "Las operaciones con archivos se detendrán y no se reanudarán automáticamente. Las eliminaciones o movimientos parciales permanecen aplicados. La indexación semántica se conciliará al reiniciar.",
    operations: "Las operaciones con archivos se detendrán y no se reanudarán automáticamente. Las eliminaciones o movimientos parciales permanecen aplicados.",
    indexing: "La indexación semántica se detendrá y conciliará las carpetas incluidas al reiniciar.",
    quit_anyway: "Salir de todos modos",
    keep_working: "Seguir trabajando",
};

const IT: ExitDialogText = ExitDialogText {
    title: "Lavoro in background in corso",
    operations_and_indexing: "Le operazioni sui file si interromperanno e non riprenderanno automaticamente. Le eliminazioni o gli spostamenti parziali restano applicati. L'indicizzazione semantica verrà riallineata al riavvio.",
    operations: "Le operazioni sui file si interromperanno e non riprenderanno automaticamente. Le eliminazioni o gli spostamenti parziali restano applicati.",
    indexing: "L'indicizzazione semantica si interromperà e riallineerà le cartelle incluse al riavvio.",
    quit_anyway: "Esci comunque",
    keep_working: "Continua a lavorare",
};

const PT: ExitDialogText = ExitDialogText {
    title: "Há trabalho em segundo plano",
    operations_and_indexing: "As operações com ficheiros serão interrompidas e não serão retomadas automaticamente. As eliminações ou movimentações parciais mantêm-se aplicadas. A indexação semântica será reconciliada ao reiniciar.",
    operations: "As operações com ficheiros serão interrompidas e não serão retomadas automaticamente. As eliminações ou movimentações parciais mantêm-se aplicadas.",
    indexing: "A indexação semântica será interrompida e reconciliará as pastas incluídas ao reiniciar.",
    quit_anyway: "Sair mesmo assim",
    keep_working: "Continuar a trabalhar",
};

const PL: ExitDialogText = ExitDialogText {
    title: "Trwa praca w tle",
    operations_and_indexing: "Operacje na plikach zostaną zatrzymane i nie zostaną wznowione automatycznie. Częściowe usunięcia lub przeniesienia pozostają wykonane. Indeksowanie semantyczne zostanie uzgodnione po ponownym uruchomieniu.",
    operations: "Operacje na plikach zostaną zatrzymane i nie zostaną wznowione automatycznie. Częściowe usunięcia lub przeniesienia pozostają wykonane.",
    indexing: "Indeksowanie semantyczne zostanie zatrzymane, a uwzględnione foldery zostaną uzgodnione po ponownym uruchomieniu.",
    quit_anyway: "Mimo to zakończ",
    keep_working: "Kontynuuj pracę",
};

/// The dialog text for the user's language.
pub(crate) const fn text_for(language: LanguageDto) -> &'static ExitDialogText {
    match language {
        LanguageDto::En => &EN,
        LanguageDto::Nl => &NL,
        LanguageDto::De => &DE,
        LanguageDto::Fr => &FR,
        LanguageDto::Es => &ES,
        LanguageDto::It => &IT,
        LanguageDto::Pt => &PT,
        LanguageDto::Pl => &PL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_distinct_translated_labels() {
        let languages = [
            LanguageDto::Nl,
            LanguageDto::De,
            LanguageDto::Fr,
            LanguageDto::Es,
            LanguageDto::It,
            LanguageDto::Pt,
            LanguageDto::Pl,
        ];
        for language in languages {
            let text = text_for(language);
            assert_ne!(text, &EN, "{language:?} must not fall back to English");
            assert_ne!(text.quit_anyway, text.keep_working);
            assert!(!text.title.is_empty() && !text.indexing.is_empty());
        }
    }
}
