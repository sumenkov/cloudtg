# CloudTG

Файловый менеджер поверх Telegram-хранилища.

- UI: React + Vite
- Desktop: Tauri
- Backend: Rust
- Локальная БД: SQLite + миграции (`src-tauri/migrations`)
- Метаданные в Telegram: теги `#ocltg #v1` (модуль `fsmeta`)
- Портативность: всё рядом с бинарём: `./data`, `./cache`, `./logs`

> Для реальной работы с Telegram нужен TDLib (libtdjson).
> API_ID, API_HASH и путь к TDLib задаются в настройках приложения и хранятся в локальной базе.
> Библиотеку можно положить рядом с бинарём или указать путь через настройку.

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

Откроется окно CloudTG. При первом запуске создадутся папки:
- `./data` (SQLite: `cloudtg.sqlite`)
- `./cache`
- `./logs`

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
