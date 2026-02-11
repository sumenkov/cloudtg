# CloudTG

Telegram как файловое хранилище с папками: файлы живут в канале, а на компьютере лежит индекс (SQLite) и кеш для быстрого открытия.
Да, это странно. Нет, это не шутка.

Зачем: чтобы складывать файлы в Telegram, но пользоваться ими как обычной папкой (с деревом, поиском и кнопкой “Открыть”).

CloudTG создаёт (или использует) два канала:
- **CloudTG**: основное хранилище файлов и структуры.
- **CloudTG Backups**: бэкапы локальной базы.

## Скачать и запустить
- Windows (portable `.zip`): [CloudTG-windows-portable-x86_64.zip](https://github.com/sumenkov/cloudtg/releases/latest/download/CloudTG-windows-portable-x86_64.zip)
- Linux (`.AppImage`): [CloudTG-linux-x86_64.AppImage](https://github.com/sumenkov/cloudtg/releases/latest/download/CloudTG-linux-x86_64.AppImage)
- macOS Intel (`.dmg`): [CloudTG-macos-x86_64.dmg](https://github.com/sumenkov/cloudtg/releases/latest/download/CloudTG-macos-x86_64.dmg)
- macOS Apple Silicon (`.dmg`): [CloudTG-macos-aarch64.dmg](https://github.com/sumenkov/cloudtg/releases/latest/download/CloudTG-macos-aarch64.dmg)
- Все релизы: [github.com/sumenkov/cloudtg/releases](https://github.com/sumenkov/cloudtg/releases)

Если что-то сломалось или есть вопрос:
- GitHub Issues: https://github.com/sumenkov/cloudtg/issues
- Логи: `./logs/cloudtg.jsonl.YYYY-MM-DD` (portable) или `CLOUDTG_STORAGE_DIR/logs` (Linux/macOS)

## Быстрый старт (примерно 30 секунд)
1. Скачай релиз, распакуй и запусти CloudTG.
2. Нажми `Настройки` → `Подключение Telegram`.
3. Введи `API_ID` и `API_HASH` (см. следующий раздел).
4. Выбери, как хранить ключи:
   - **Системное хранилище** (keychain ОС).
   - **Зашифрованный файл** (нужен пароль).
   - Или выключи `Запомнить ключи` (только текущий запуск, ничего не сохраняем).
5. TDLib:
   - оставь путь пустым, CloudTG попробует найти/скачать/собрать TDLib автоматически;
   - или нажми `Открыть` рядом с полем и укажи файл `tdjson.dll` / `libtdjson.*`.
6. Нажми `Сохранить подключение`.
7. На главном экране пройди авторизацию Telegram и нажми `Обновить сейчас`.

Подсказка: если ключи сохранены в зашифрованном файле, CloudTG попросит пароль прямо на экране входа (без прыжков по меню).

## Где взять API_ID и API_HASH
1. Открой `my.telegram.org` и войди по номеру телефона.
2. Перейди в **API development tools**.
3. Создай приложение (любое имя, описание можно честное: “CloudTG, пожалуйста, работай”).
4. Скопируй **API ID** и **API Hash** и введи их в CloudTG.

Важно:
- не публикуй `API_HASH` (ни в репозиторий, ни в issue, ни в скриншоты).
- если собирать с `CLOUDTG_EMBED_API_KEYS=1`, считай ключи публичными (их можно вытащить из бинарника).

## Что умеет
- Хранит файлы в Telegram-канале CloudTG и показывает их как дерево папок.
- Загружает, скачивает и открывает файлы.
- Не делает лишние локальные копии и показывает `Открыть папку` только там, где файл уже скачан.
- Перемещает и удаляет файлы, удаляет пустые папки.
- Делится файлом в любой чат через действие `Поделиться`.
- Ищет по имени и расширению.
- Проверяет целостность и помечает битые записи; умеет восстанавливать.
- Делает бэкап SQLite в канал CloudTG Backups и умеет восстановиться при запуске.
- Подготавливает TDLib автоматически (поиск → скачивание → сборка).

## Как пользоваться
- На главном экране кнопка `Обновить сейчас` подтягивает новые сообщения из канала и обновляет дерево.
- Слева дерево папок; справа вкладки `Файлы`, `Папки`, `Поиск`, `Сервис`.
- Вкладка `Файлы`:
  - `Выбрать и загрузить` загружает файлы в выбранную папку.
  - Кнопка у файла: `Скачать` или `Открыть` (если локальная копия уже есть).
  - Меню `⋯ Действия`: `Поделиться`, `Открыть папку`, `Скачать заново`, `Восстановить` (для битых), `Удалить`.
- Вкладка `Поиск`: по имени и/или расширению, в текущей папке или по всем папкам.

## Бэкап и восстановление
- `Создать бэкап` отправляет текущую SQLite в канал **CloudTG Backups**.
- `Восстановить базу из бэкапа` использует последний бэкап; если он старее канала хранения, база пересобирается из сообщений.
- После восстановления нужен перезапуск приложения.

## Где лежат данные и логи
По умолчанию CloudTG хранит всё рядом с бинарём:
- `./data` (SQLite: `cloudtg.sqlite`).
- `./cache` (включая локальные скачанные файлы).
- `./logs` (JSON Lines).

Linux/macOS: если рядом с бинарём нет прав на запись, CloudTG перенесёт `data/cache/logs` в пользовательскую директорию:
- Linux: `$XDG_DATA_HOME/cloudtg` или `~/.local/share/cloudtg`.
- macOS: `~/Library/Application Support/CloudTG`.

Переопределение:
- `CLOUDTG_BASE_DIR` задаёт базовую директорию (все `data/cache/logs` будут внутри неё).
- Linux/macOS: `CLOUDTG_STORAGE_DIR` задаёт директорию хранения.

## TDLib (libtdjson / tdjson.dll)
Без TDLib CloudTG работать не сможет.

Как CloudTG ищет библиотеку (упрощённо):
1. Путь из настроек (`Путь к TDLib`).
2. `CLOUDTG_TDLIB_PATH`.
3. Рядом с бинарём (portable).
4. В ресурсах приложения `src-tauri/resources/tdlib/...` (для сборок/CI).
5. Если не нашлось: пытается скачать предсобранную TDLib из GitHub Releases (по `tdlib-manifest.json`).
6. Если не вышло: пробует собрать TDLib в `./third_party/tdlib` (нужны `git`, `cmake` и C/C++ toolchain).

### Если на Windows видно “LoadLibraryExW failed”
Обычно это значит “DLL нашли, но не загрузили”: не та разрядность, рядом нет зависимых DLL или в системе нет нужных рантаймов.

Что сделать по-быстрому:
1. Возьми TDLib из релиза CloudTG (архив обычно содержит всё нужное) и распакуй так, чтобы все `.dll` лежали рядом с `CloudTG.exe`.
2. Либо укажи путь к `tdjson.dll` через кнопку `Открыть`.
3. Если всё равно не заводится, приложи лог (см. раздел ниже).

### Скачать предсобранную TDLib вручную
Через npm (автоопределение ОС):
```bash
npm run tdlib:fetch
```

Linux/macOS:
```bash
./scripts/fetch-tdlib.sh
```

Windows (PowerShell):
```powershell
.\scripts\fetch-tdlib.ps1
```

Переменные для автоскачивания:
- `CLOUDTG_TDLIB_REPO=owner/repo` (если автоопределение репозитория не сработало).
- `CLOUDTG_TDLIB_MANIFEST_URL=.../tdlib-manifest.json` (прямая ссылка на манифест).
- `GITHUB_TOKEN` / `GH_TOKEN` (для приватных репозиториев).

## Разработка
### Требования
- Node.js 18+ (или 20+).
- Rust stable (edition 2021).
- Для реальной работы с Telegram нужен TDLib (см. выше).

### Запуск в dev
```bash
npm install
npm run tauri:dev
```

### Сборка
```bash
npm run tauri:build
```

### Тесты
```bash
npm test
npm run rust:test
```

<details>
<summary>Системные зависимости Tauri (Linux/macOS/Windows)</summary>

Linux (Debian/Ubuntu):
```bash
sudo apt update
sudo apt install -y pkg-config libglib2.0-dev libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf
```
Если видишь ошибку вида `glib-sys ... pkg-config`, значит не хватает `pkg-config` и/или `libglib2.0-dev`.

macOS:
```bash
xcode-select --install
```
Если сборка ругается на `pkg-config`, установи:
```bash
brew install pkg-config
```

Windows 10/11:
- Нужен WebView2 Runtime (обычно уже установлен в системе).
- Нужны Visual Studio Build Tools с компонентом C++ и Windows SDK.
- Через winget можно поставить инсталлер Build Tools:
```powershell
winget install Microsoft.VisualStudio.2022.BuildTools
```

</details>

<details>
<summary>Варианты сборки (вшитые ключи vs ввод в UI)</summary>

По умолчанию ключи вводятся в UI и сохраняются в системном хранилище или в зашифрованном файле.
Для dev/CI можно передать их через env или `.env` (см. `.env.example`).

Если очень нужно вшить ключи в бинарник:
- `CLOUDTG_EMBED_API_KEYS=1`
- `CLOUDTG_API_ID`, `CLOUDTG_API_HASH`

Пример:
```bash
CLOUDTG_EMBED_API_KEYS=1 CLOUDTG_API_ID=123 CLOUDTG_API_HASH=... npm run tauri:build
```

Важно: вшитые ключи можно извлечь из бинарника, поэтому считай их публичными.

</details>

## Безопасность и хранение ключей (коротко)
- CloudTG не хранит `API_ID/API_HASH` в SQLite.
- Ключи можно хранить в системном keychain (`keyring`) или в зашифрованном файле `data/secrets/tg_keys.enc.json`.
- Для шифрования используется Argon2 + `XChaCha20-Poly1305` (AEAD).
- Режим `только в текущем запуске` не пишет ключи на диск.

<details>
<summary>Threat model (подробно)</summary>

**Защищаемые активы**
- `API_ID` и `API_HASH`.
- Локальная Telegram-сессия TDLib и служебные данные приложения.
- Локальные копии файлов в `cache/downloads`.
- Метаданные структуры в SQLite (имена, дерево, `tg_chat_id`, `tg_msg_id`).

**Границы доверия**
- Доверенная зона: текущий пользователь ОС, локальная ФС с корректными правами, системное хранилище ключей ОС.
- Недоверенная зона: сеть, внешние репозитории/релизы, любые третьи лица с доступом к бинарнику или рабочей директории.

**Модель нарушителя**
- Имеет доступ к репозиторию, артефакту сборки или логам.
- Имеет чтение пользовательских файлов (или запускает процесс от имени пользователя).
- Не имеет контроля над твоим Telegram-аккаунтом по умолчанию.

**Что делаем для защиты**
- `API_ID/API_HASH` не хранятся в SQLite.
- Ключи можно хранить в системном keychain (`keyring`) или в зашифрованном файле:
  `data/secrets/tg_keys.enc.json`.
- Для шифрования используется KDF Argon2 + `XChaCha20-Poly1305` (AEAD).
- Запись зашифрованного файла выполняется атомарно (`write + rename`), на Unix выставляются права `0600`.
- Режим хранения `только в текущем запуске` не пишет ключи на диск.

**Что не покрывается**
- Компрометация хоста (malware, keylogger, дамп памяти, root/admin-доступ).
- Утечка ключей из переменных окружения, CI-логов или истории shell.
- Обратная инженерия бинарника при сборке с `CLOUDTG_EMBED_API_KEYS=1`.
- Риски Telegram-каналов при ошибочной выдаче доступа посторонним.

**Практические рекомендации**
- Для постоянного использования выбирай keychain или encrypted file с сильным паролем.
- Не используй `CLOUDTG_EMBED_API_KEYS=1` в продакшн-сборках.
- Для dev/CI через env не публикуй значения в логах, `.env`, скриншотах и issue.
- Если есть подозрение на утечку, перевыпусти `API_HASH` на `my.telegram.org`.
- Ограничь доступ к каналам CloudTG и CloudTG Backups только доверенными участниками.

</details>

## Сообщить об ошибке / попросить помощи
- GitHub Issues: https://github.com/sumenkov/cloudtg/issues
- Что приложить (без `API_HASH`, пожалуйста):
  - ОС и версия CloudTG.
  - шаги, как воспроизвести.
  - лог из `./logs/cloudtg.jsonl.YYYY-MM-DD` (или из `CLOUDTG_STORAGE_DIR/logs` на Linux/macOS).

## Технологии (для любопытных)
- UI: React + Vite.
- Desktop: Tauri.
- Backend: Rust.
- Локальная БД: SQLite + миграции (`src-tauri/migrations`).
- Метаданные в Telegram: теги `#ocltg #v1` (модуль `fsmeta`).

<details>
<summary>Формат тегов (fsmeta)</summary>

Файл (caption):
`#ocltg #v1 #file d=<dirId> f=<fileId> n=<name> h=<hashShort>`

Директория (text msg):
`#ocltg #v1 #dir d=<dirId> p=<ROOT|parentId> name=<folderName>`

</details>
