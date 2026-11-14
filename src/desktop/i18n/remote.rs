//! Remote dictation interface strings; not yet reviewed by native speakers.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Confirm remove", ["Potwierdź usunięcie", "确认移除", "削除を確認", "Entfernen bestätigen", "Confirmar eliminación", "Підтвердити видалення"]),
    ("Remove…", ["Usuń…", "移除…", "削除…", "Entfernen…", "Eliminar…", "Видалити…"]),
    ("Remote dictation", ["Zdalne dyktowanie", "远程听写", "リモート音声入力", "Ferndiktat", "Dictado remoto", "Віддалене диктування"]),
    ("Dictate from a paired phone over Tailscale", ["Dyktuj ze sparowanego telefonu przez Tailscale", "通过 Tailscale 从已配对的手机听写", "ペアリングしたスマートフォンからTailscale経由で音声入力", "Mit einem gekoppelten Telefon über Tailscale diktieren", "Dicta desde un teléfono vinculado mediante Tailscale", "Диктуйте зі сполученого телефона через Tailscale"]),
    ("Address", ["Adres", "地址", "アドレス", "Adresse", "Dirección", "Адреса"]),
    ("Paired devices", ["Sparowane urządzenia", "已配对的设备", "ペアリング済みデバイス", "Gekoppelte Geräte", "Dispositivos vinculados", "Сполучені пристрої"]),
    ("Pair device", ["Sparuj urządzenie", "配对设备", "デバイスをペアリング", "Gerät koppeln", "Vincular dispositivo", "Сполучити пристрій"]),
    ("Process transcripts from the phone", ["Przetwarzaj transkrypcje z telefonu", "处理手机发来的转录", "スマートフォンの文字起こしを処理", "Transkripte vom Telefon verarbeiten", "Procesar transcripciones del teléfono", "Обробляти транскрипції з телефона"]),
    ("If the phone cannot connect, allow this port only on the Tailscale interface in Windows Firewall.", ["Jeśli telefon nie może się połączyć, zezwól na ten port tylko w interfejsie Tailscale w Zaporze Windows.", "如果手机无法连接，请在 Windows 防火墙中仅为 Tailscale 接口允许此端口。", "スマートフォンが接続できない場合は、WindowsファイアウォールでTailscaleインターフェースのこのポートのみを許可してください。", "Wenn das Telefon keine Verbindung herstellen kann, diesen Port in der Windows-Firewall nur für die Tailscale-Schnittstelle freigeben.", "Si el teléfono no se conecta, permite este puerto solo en la interfaz Tailscale del Firewall de Windows.", "Якщо телефон не підключається, дозвольте цей порт лише для інтерфейсу Tailscale у брандмауері Windows."]),
    ("Pairing code expires in five minutes", ["Kod parowania wygasa za pięć minut", "配对码将在五分钟后过期", "ペアリングコードは5分で期限切れになります", "Kopplungscode läuft in fünf Minuten ab", "El código de vinculación caduca en cinco minutos", "Код сполучення дійсний п’ять хвилин"]),
    ("Pairing payload", ["Dane parowania", "配对数据", "ペアリングデータ", "Kopplungsdaten", "Datos de vinculación", "Дані сполучення"]),
    ("Pairing QR code", ["Kod QR parowania", "配对二维码", "ペアリングQRコード", "QR-Code zur Kopplung", "Código QR de vinculación", "QR-код сполучення"]),
    ("No Tailscale address", ["Brak adresu Tailscale", "没有 Tailscale 地址", "Tailscaleアドレスがありません", "Keine Tailscale-Adresse", "Sin dirección Tailscale", "Немає адреси Tailscale"]),
    ("Remote listener unavailable", ["Zdalny odbiornik niedostępny", "远程监听不可用", "リモートリスナーを利用できません", "Remote-Listener nicht verfügbar", "Receptor remoto no disponible", "Віддалений слухач недоступний"]),
    ("Remote device store unavailable", ["Magazyn urządzeń niedostępny", "远程设备存储不可用", "リモートデバイスストアを利用できません", "Gerätespeicher nicht verfügbar", "Almacén de dispositivos no disponible", "Сховище пристроїв недоступне"]),
    ("Could not bind the Tailscale address and port", ["Nie można otworzyć adresu i portu Tailscale", "无法绑定 Tailscale 地址和端口", "Tailscaleのアドレスとポートを使用できません", "Tailscale-Adresse und Port konnten nicht gebunden werden", "No se pudo abrir la dirección y el puerto Tailscale", "Не вдалося відкрити адресу й порт Tailscale"]),
];

#[rustfmt::skip]
const EXTRA: &[(&str, [&str; 3])] = &[
    ("Confirm remove", ["Confirmar remoção", "Confirmer le retrait", "Подтвердить удаление"]),
    ("Remove…", ["Remover…", "Retirer…", "Удалить…"]),
    ("Remote dictation", ["Ditado remoto", "Dictée à distance", "Удалённая диктовка"]),
    ("Dictate from a paired phone over Tailscale", ["Dite de um telefone pareado pelo Tailscale", "Dictez depuis un téléphone associé via Tailscale", "Диктуйте с сопряжённого телефона через Tailscale"]),
    ("Address", ["Endereço", "Adresse", "Адрес"]),
    ("Paired devices", ["Dispositivos pareados", "Appareils associés", "Сопряжённые устройства"]),
    ("Pair device", ["Parear dispositivo", "Associer un appareil", "Подключить устройство"]),
    ("Process transcripts from the phone", ["Processar transcrições do telefone", "Traiter les transcriptions du téléphone", "Обрабатывать расшифровки с телефона"]),
    ("If the phone cannot connect, allow this port only on the Tailscale interface in Windows Firewall.", ["Se o telefone não conectar, permita esta porta apenas na interface Tailscale no Firewall do Windows.", "Si le téléphone ne se connecte pas, autorisez ce port uniquement sur l’interface Tailscale dans le pare-feu Windows.", "Если телефон не подключается, разрешите этот порт только для интерфейса Tailscale в брандмауэре Windows."]),
    ("Pairing code expires in five minutes", ["O código de pareamento expira em cinco minutos", "Le code d’association expire dans cinq minutes", "Код сопряжения действует пять минут"]),
    ("Pairing payload", ["Dados de pareamento", "Données d’association", "Данные сопряжения"]),
    ("Pairing QR code", ["Código QR de pareamento", "Code QR d’association", "QR-код сопряжения"]),
    ("No Tailscale address", ["Sem endereço Tailscale", "Aucune adresse Tailscale", "Нет адреса Tailscale"]),
    ("Remote listener unavailable", ["Receptor remoto indisponível", "Écoute à distance indisponible", "Удалённый приёмник недоступен"]),
    ("Remote device store unavailable", ["Armazenamento de dispositivos indisponível", "Stockage des appareils indisponible", "Хранилище устройств недоступно"]),
    ("Could not bind the Tailscale address and port", ["Não foi possível abrir o endereço e a porta Tailscale", "Impossible d’ouvrir l’adresse et le port Tailscale", "Не удалось открыть адрес и порт Tailscale"]),
];

pub(super) fn extra(column: usize) -> impl Iterator<Item = (&'static str, &'static str)> {
    EXTRA
        .iter()
        .map(move |(key, values)| (*key, values[column]))
}
