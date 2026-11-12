//! Antigravity account and summaries strings. Extra columns are pt, fr, ru.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
("Written by Antigravity ({model}). {details}", ["Napisane przez Antigravity ({model}). {details}", "由 Antigravity（{model}）撰写。{details}", "Antigravity（{model}）で作成。{details}", "Von Antigravity ({model}) geschrieben. {details}", "Escrito por Antigravity ({model}). {details}", "Написано Antigravity ({model}). {details}"]),
("The summary writer answered without JSON", ["Model podsumowań odpowiedział bez JSON", "摘要模型未返回 JSON", "要約モデルが JSON を返しませんでした", "Das Zusammenfassungsmodell hat ohne JSON geantwortet", "El modelo de resumen respondió sin JSON", "Модель підсумків відповіла без JSON"]),
("The summary writer answered invalid JSON", ["Model podsumowań zwrócił nieprawidłowy JSON", "摘要模型返回了无效的 JSON", "要約モデルが無効な JSON を返しました", "Das Zusammenfassungsmodell hat ungültiges JSON geliefert", "El modelo de resumen devolvió JSON no válido", "Модель підсумків повернула некоректний JSON"]),
("Writing a title and summary with Antigravity…", ["Piszę tytuł i podsumowanie za pomocą Antigravity…", "正在通过 Antigravity 撰写标题和摘要…", "Antigravity でタイトルと要約を作成中…", "Titel und Zusammenfassung werden mit Antigravity erstellt…", "Escribiendo un título y resumen con Antigravity…", "Створення назви та підсумку через Antigravity…"]),
("Antigravity", ["Antigravity", "Antigravity", "Antigravity", "Antigravity", "Antigravity", "Antigravity"]),
("Antigravity summaries", ["Podsumowania Antigravity", "Antigravity 摘要", "Antigravity の要約", "Antigravity-Zusammenfassungen", "Resúmenes de Antigravity", "Підсумки Antigravity"]),
("Write meeting summaries with your Google Antigravity account", ["Twórz podsumowania spotkań za pomocą konta Google Antigravity", "使用你的 Google Antigravity 账号撰写会议摘要", "Google Antigravity アカウントで会議の要約を作成します", "Meeting-Zusammenfassungen mit deinem Google-Antigravity-Konto erstellen", "Escribe resúmenes de reuniones con tu cuenta de Google Antigravity", "Створюйте підсумки зустрічей через свій обліковий запис Google Antigravity"]),
("Sends the transcript to your Google account's Antigravity", ["Wysyła transkrypcję do Antigravity na Twoim koncie Google", "将转录文本发送到你 Google 账号的 Antigravity", "文字起こしを Google アカウントの Antigravity に送信します", "Sendet das Transkript an Antigravity in deinem Google-Konto", "Envía la transcripción a Antigravity de tu cuenta de Google", "Надсилає розшифровку до Antigravity вашого облікового запису Google"]),
("Sign in with Google", ["Zaloguj się przez Google", "使用 Google 登录", "Google でログイン", "Mit Google anmelden", "Iniciar sesión con Google", "Увійти через Google"]),
("Sign out", ["Wyloguj się", "退出登录", "ログアウト", "Abmelden", "Cerrar sesión", "Вийти"]),
("Sign in to Antigravity", ["Zaloguj się do Antigravity", "登录 Antigravity", "Antigravity にログイン", "Bei Antigravity anmelden", "Inicia sesión en Antigravity", "Увійдіть в Antigravity"]),
("Antigravity sign-in failed. Try again", ["Logowanie do Antigravity nie powiodło się. Spróbuj ponownie", "Antigravity 登录失败，请重试", "Antigravity にログインできませんでした。もう一度お試しください", "Anmeldung bei Antigravity fehlgeschlagen. Erneut versuchen", "Error al iniciar sesión en Antigravity. Inténtalo de nuevo", "Не вдалося увійти в Antigravity. Спробуйте ще раз"]),
("Turn on Antigravity summaries in Labs", ["Włącz podsumowania Antigravity w Labs", "在 Labs 中开启 Antigravity 摘要", "Labs で Antigravity の要約をオンにしてください", "Antigravity-Zusammenfassungen in Labs einschalten", "Activa los resúmenes de Antigravity en Labs", "Увімкніть підсумки Antigravity в Labs"]),
("Sign in to Antigravity again", ["Zaloguj się ponownie do Antigravity", "请重新登录 Antigravity", "Antigravity に再ログインしてください", "Erneut bei Antigravity anmelden", "Vuelve a iniciar sesión en Antigravity", "Увійдіть в Antigravity ще раз"]),
("Antigravity quota reached. Try again later", ["Wyczerpano limit Antigravity. Spróbuj później", "Antigravity 配额已用尽，请稍后重试", "Antigravity の利用上限に達しました。後でお試しください", "Antigravity-Kontingent erreicht. Später erneut versuchen", "Se alcanzó la cuota de Antigravity. Inténtalo más tarde", "Вичерпано квоту Antigravity. Спробуйте пізніше"]),
("Antigravity request cancelled", ["Anulowano żądanie Antigravity", "Antigravity 请求已取消", "Antigravity のリクエストをキャンセルしました", "Antigravity-Anfrage abgebrochen", "Solicitud de Antigravity cancelada", "Запит Antigravity скасовано"]),
("Antigravity request timed out", ["Upłynął czas żądania Antigravity", "Antigravity 请求超时", "Antigravity のリクエストがタイムアウトしました", "Zeitüberschreitung bei Antigravity-Anfrage", "Se agotó el tiempo de la solicitud de Antigravity", "Час очікування запиту Antigravity минув"]),
("Antigravity request failed", ["Żądanie Antigravity nie powiodło się", "Antigravity 请求失败", "Antigravity のリクエストに失敗しました", "Antigravity-Anfrage fehlgeschlagen", "La solicitud de Antigravity falló", "Не вдалося виконати запит Antigravity"]),
("Sign-in complete. Return to Hex.", ["Logowanie zakończone. Wróć do Hex.", "登录完成，请返回 Hex。", "ログインが完了しました。Hex に戻ってください。", "Anmeldung abgeschlossen. Zu Hex zurückkehren.", "Inicio de sesión completado. Vuelve a Hex.", "Вхід завершено. Поверніться до Hex."]),
("Sign-in failed. Return to Hex.", ["Logowanie nie powiodło się. Wróć do Hex.", "登录失败，请返回 Hex。", "ログインできませんでした。Hex に戻ってください。", "Anmeldung fehlgeschlagen. Zu Hex zurückkehren.", "Error al iniciar sesión. Vuelve a Hex.", "Не вдалося увійти. Поверніться до Hex."]),
("Antigravity sign-in port 51121 is already in use", ["Port logowania Antigravity 51121 jest już zajęty", "Antigravity 登录端口 51121 已被占用", "Antigravity のログイン用ポート 51121 は使用中です", "Der Antigravity-Anmeldeport 51121 ist bereits belegt", "El puerto de inicio de sesión de Antigravity 51121 ya está en uso", "Порт входу Antigravity 51121 уже зайнятий"]),
("Could not open Google sign-in in your browser", ["Nie udało się otworzyć logowania Google w przeglądarce", "无法在浏览器中打开 Google 登录", "ブラウザで Google ログインを開けませんでした", "Google-Anmeldung konnte nicht im Browser geöffnet werden", "No se pudo abrir el inicio de sesión de Google en el navegador", "Не вдалося відкрити вхід Google у браузері"]),
("Antigravity account email unavailable", ["Adres e-mail konta Antigravity jest niedostępny", "无法获取 Antigravity 账号邮箱", "Antigravity アカウントのメールアドレスを取得できません", "E-Mail-Adresse des Antigravity-Kontos nicht verfügbar", "El correo de la cuenta de Antigravity no está disponible", "Електронна адреса облікового запису Antigravity недоступна"]),
("Google sign-in refused", ["Google odmówił logowania", "Google 拒绝了登录", "Google がログインを拒否しました", "Google hat die Anmeldung abgelehnt", "Google rechazó el inicio de sesión", "Google відхилив вхід"]),
("Antigravity stopped at its length limit", ["Antigravity zatrzymał się na limicie długości", "Antigravity 已达到长度上限", "Antigravity が長さの上限で停止しました", "Antigravity hat an seiner Längengrenze angehalten", "Antigravity se detuvo en su límite de longitud", "Antigravity зупинився на межі довжини"]),
("Antigravity client secret missing", ["Brak sekretu klienta Antigravity", "缺少 Antigravity 客户端密钥", "Antigravity のクライアントシークレットがありません", "Antigravity-Client-Secret fehlt", "Falta el secreto de cliente de Antigravity", "Відсутній секрет клієнта Antigravity"]),
("Open this address to sign in:", ["Otwórz ten adres, aby się zalogować:", "打开此地址进行登录：", "ログインするにはこのアドレスを開いてください:", "Öffne diese Adresse, um dich anzumelden:", "Abre esta dirección para iniciar sesión:", "Відкрийте цю адресу, щоб увійти:"]),
];

#[rustfmt::skip]
const EXTRA: &[(&str, [&str; 3])] = &[
("Written by Antigravity ({model}). {details}", ["Escrito pelo Antigravity ({model}). {details}", "Rédigé par Antigravity ({model}). {details}", "Создано Antigravity ({model}). {details}"]),
("The summary writer answered without JSON", ["O modelo de resumo respondeu sem JSON", "Le modèle de résumé a répondu sans JSON", "Модель сводки ответила без JSON"]),
("The summary writer answered invalid JSON", ["O modelo de resumo retornou JSON inválido", "Le modèle de résumé a renvoyé un JSON non valide", "Модель сводки вернула некорректный JSON"]),
("Writing a title and summary with Antigravity…", ["Escrevendo título e resumo com Antigravity…", "Rédaction du titre et du résumé avec Antigravity…", "Создание заголовка и сводки через Antigravity…"]),
("Antigravity", ["Antigravity", "Antigravity", "Antigravity"]),
("Antigravity summaries", ["Resumos do Antigravity", "Résumés Antigravity", "Сводки Antigravity"]),
("Write meeting summaries with your Google Antigravity account", ["Escreva resumos de reuniões com sua conta do Google Antigravity", "Rédiger des résumés de réunion avec votre compte Google Antigravity", "Создавайте сводки встреч через свой аккаунт Google Antigravity"]),
("Sends the transcript to your Google account's Antigravity", ["Envia a transcrição ao Antigravity da sua conta Google", "Envoie la transcription à Antigravity de votre compte Google", "Отправляет расшифровку в Antigravity вашего аккаунта Google"]),
("Sign in with Google", ["Entrar com Google", "Se connecter avec Google", "Войти через Google"]),
("Sign out", ["Sair", "Se déconnecter", "Выйти"]),
("Sign in to Antigravity", ["Entre no Antigravity", "Connectez-vous à Antigravity", "Войдите в Antigravity"]),
("Antigravity sign-in failed. Try again", ["Falha ao entrar no Antigravity. Tente novamente", "Échec de connexion à Antigravity. Réessayez", "Не удалось войти в Antigravity. Повторите попытку"]),
("Turn on Antigravity summaries in Labs", ["Ative os resumos do Antigravity em Labs", "Activez les résumés Antigravity dans Labs", "Включите сводки Antigravity в Labs"]),
("Sign in to Antigravity again", ["Entre no Antigravity novamente", "Reconnectez-vous à Antigravity", "Войдите в Antigravity снова"]),
("Antigravity quota reached. Try again later", ["Cota do Antigravity atingida. Tente mais tarde", "Quota Antigravity atteint. Réessayez plus tard", "Квота Antigravity исчерпана. Попробуйте позже"]),
("Antigravity request cancelled", ["Solicitação do Antigravity cancelada", "Requête Antigravity annulée", "Запрос Antigravity отменён"]),
("Antigravity request timed out", ["Tempo limite da solicitação do Antigravity esgotado", "Délai de la requête Antigravity dépassé", "Время ожидания запроса Antigravity истекло"]),
("Antigravity request failed", ["Falha na solicitação do Antigravity", "Échec de la requête Antigravity", "Запрос Antigravity завершился ошибкой"]),
("Sign-in complete. Return to Hex.", ["Login concluído. Volte ao Hex.", "Connexion terminée. Revenez à Hex.", "Вход выполнен. Вернитесь в Hex."]),
("Sign-in failed. Return to Hex.", ["Falha no login. Volte ao Hex.", "Échec de connexion. Revenez à Hex.", "Не удалось войти. Вернитесь в Hex."]),
("Antigravity sign-in port 51121 is already in use", ["A porta de login do Antigravity 51121 já está em uso", "Le port de connexion Antigravity 51121 est déjà utilisé", "Порт входа Antigravity 51121 уже занят"]),
("Could not open Google sign-in in your browser", ["Não foi possível abrir o login do Google no navegador", "Impossible d'ouvrir la connexion Google dans le navigateur", "Не удалось открыть вход Google в браузере"]),
("Antigravity account email unavailable", ["E-mail da conta Antigravity indisponível", "Adresse e-mail du compte Antigravity indisponible", "Почта аккаунта Antigravity недоступна"]),
("Google sign-in refused", ["O Google recusou o login", "Google a refusé la connexion", "Google отклонил вход"]),
("Antigravity stopped at its length limit", ["O Antigravity parou no limite de tamanho", "Antigravity s'est arrêté à sa limite de longueur", "Antigravity остановился на пределе длины"]),
("Antigravity client secret missing", ["Segredo do cliente Antigravity ausente", "Secret client Antigravity manquant", "Отсутствует секрет клиента Antigravity"]),
("Open this address to sign in:", ["Abra este endereço para entrar:", "Ouvrez cette adresse pour vous connecter :", "Откройте этот адрес, чтобы войти:"]),
];

pub(super) fn extra(column: usize) -> impl Iterator<Item = (&'static str, &'static str)> {
    EXTRA
        .iter()
        .map(move |(key, values)| (*key, values[column]))
}
