# CloudTG

Файловый менеджер поверх Telegram‑хранилища.  
Хранит структуру папок и метаданные в Telegram, а локально — SQLite.

## Возможности
- Авторизация в Telegram через TDLib
- Хранилище в Telegram‑канале CloudTG
- Дерево директорий с вложенностью и быстрым созданием папок
- Автосборка TDLib при отсутствии библиотеки
- Предсобранные TDLib‑артефакты для быстрого старта
- Логи в формате JSON Lines для последующего парсинга

## Технологии
- UI: React + Vite
- Desktop: Tauri
- Backend: Rust
- Локальная БД: SQLite + миграции (`src-tauri/migrations`)
- Метаданные в Telegram: теги `#ocltg #v1` (модуль `fsmeta`)
- Портативность: по умолчанию рядом с бинарём (`./data`, `./cache`, `./logs`), на Linux/macOS при отсутствии прав — пользовательские директории

> Для реальной работы с Telegram нужен TDLib (libtdjson).
> API_ID, API_HASH и путь к TDLib задаются в настройках приложения и хранятся в локальной базе.
> Если путь к TDLib не указан и библиотека не найдена, приложение попробует скачать и собрать TDLib в `./third_party/tdlib`.
> Для автосборки нужны `git`, `cmake` и C/C++ toolchain, установленные в системе.

## Предсобранная TDLib (быстрый запуск без сборки)
Чтобы приложение запускалось сразу, положи `libtdjson` в ресурсы приложения:

```
src-tauri/resources/tdlib/<os>-<arch>/libtdjson.*
```

Примеры:
- Windows: `src-tauri/resources/tdlib/windows-x86_64/tdjson.dll`
- macOS (Apple Silicon): `src-tauri/resources/tdlib/macos-aarch64/libtdjson.dylib`
- Ubuntu Linux: `src-tauri/resources/tdlib/linux-x86_64/libtdjson.so`

Поддерживаются также папки `tdlib/<os>` и просто `tdlib/`.
При запуске приложение сначала ищет библиотеку в ресурсах и рядом с бинарём, и только затем запускает автосборку.

## CI артефакты TDLib
В репозитории есть workflow `Сборка TDLib (prebuilt)` — он собирает TDLib под Windows/macOS/Linux
и выкладывает артефакты в GitHub Actions, а при публикации релиза добавляет файлы в релиз.

## Скачать предсобранную TDLib автоматически
Linux/macOS:
```bash
./scripts/fetch-tdlib.sh
```

Windows (PowerShell):
```powershell
.\scripts\fetch-tdlib.ps1
```

Через npm (автоопределение ОС):
```bash
npm run tdlib:fetch
```

Если библиотека уже есть в `src-tauri/resources/tdlib`, скрипт просто сообщит путь и ничего не скачает.

Скрипты берут файлы из последнего релиза GitHub.
Если репозиторий не определяется автоматически, задай переменную:
`CLOUDTG_TDLIB_REPO=owner/repo`.
Для приватных репозиториев добавь `GITHUB_TOKEN` или `GH_TOKEN`.

## Быстрая установка зависимостей для автосборки TDLib
- Ubuntu/Debian:
  ```bash
  sudo apt-get install git cmake gperf build-essential
  ```
- Arch:
  ```bash
  sudo pacman -S git cmake gperf base-devel
  ```
- macOS (Homebrew):
  ```bash
  brew install git cmake gperf
  ```
- Windows (MSYS2):
  ```powershell
  pacman -S mingw-w64-x86_64-gperf mingw-w64-x86_64-cmake git
  ```

## Быстрый старт (dev)

### 1) Установи зависимости
- Node.js 18+ (или 20+)
- Rust stable (edition 2021)

Для запуска Tauri нужны системные зависимости (WebView2/Visual Studio Build Tools на Windows; webkit2gtk на Linux и т.д.).
Если на этом шаге будут ошибки, просто покажи их мне, я скажу точные пакеты под твою ОС.

### 2) Запусти
```bash
# в корне проекта
npm install
npm run tauri:dev
```

Откроется окно CloudTG. При первом запуске создадутся папки рядом с бинарём
(если нет прав — см. разделы «Данные и кеш» и «Логи»):
- `./data` (SQLite: `cloudtg.sqlite`)
- `./cache`
- `./logs`

## Настройки и кнопки
В настройках доступны основные кнопки управления Telegram‑каналом:

- **Проверить связь с Telegram** — отправляет тестовое сообщение в канал CloudTG.
- **Создать канал в Telegram** — вручную создаёт новый канал CloudTG и переносит туда данные из БД
  (директории и файлы, с обновлением `tg_msg_id` и `tg_chat_id`).

Если ты вышел из канала или потерял к нему доступ — воспользуйся кнопкой создания нового канала.

## Данные и кеш
На Linux/macOS используется общая директория хранения. Если рядом с бинарём нет прав на запись,
приложение переносит `data/cache/logs` в пользовательский каталог:
- Linux: `$XDG_DATA_HOME/cloudtg` → `~/.local/share/cloudtg`
- macOS: `~/Library/Application Support/CloudTG`

Путь можно задать вручную через переменную `CLOUDTG_STORAGE_DIR` (Linux/macOS).
На Windows всё остаётся рядом с бинарём.

## Логи
Логи пишутся в папку `./logs` в формате **JSON Lines** (одна JSON‑строка на событие).
Файл имеет вид `cloudtg.jsonl.YYYY-MM-DD` и подходит для последующего парсинга.
На Linux/macOS логи хранятся вместе с `data/cache` в общей директории
(см. раздел «Данные и кеш»). Путь можно задать через `CLOUDTG_STORAGE_DIR`.

## Как получить API_ID и API_HASH Telegram
1) Открой сайт `my.telegram.org` и войди по номеру телефона.
2) Перейди в раздел **API development tools**.
3) Создай новое приложение (любое имя/описание).
4) Скопируй значения **API ID** и **API Hash**.
5) Внеси их в настройки CloudTG.

Важно: не публикуй API_HASH и не добавляй его в репозиторий.

## Тесты

Rust:
```bash
npm run rust:test
```

UI:
```bash
npm test
```

## Сборка portable
```bash
npm run tauri:build
```

## Формат тегов (fsmeta)

Файл (caption):
`#ocltg #v1 #file d=<dirId> f=<fileId> n=<name> h=<hashShort>`

Директория (text msg):
`#ocltg #v1 #dir d=<dirId> p=<ROOT|parentId> name=<folderName>`
