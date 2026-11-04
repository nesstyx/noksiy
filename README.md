# Noksiy Chess Engine

**Самый сильный шахматный движок в мире, написанный на Rust** 🦀♟️

## О проекте

Noksiy — это высокопроизводительный шахматный движок, разработанный с нуля на языке Rust. Использует современные алгоритмы поиска и оценки позиций для достижения максимальной игровой силы.

## Особенности

- ⚡ **Bitboard representation** — молниеносная обработка позиций
- 🧠 **Alpha-Beta pruning** с улучшениями
- 🎯 **Iterative deepening** — постепенное углубление поиска
- 📊 **Piece-Square Tables** — позиционная оценка
- 🔪 **Quiescence search** — тактическая точность
- 🎲 **Move ordering** — killer moves + history heuristic
- 🎮 **UCI protocol** — совместимость со всеми шахматными GUI

## Установка

```bash
git clone https://github.com/nesstyx/noksiy.git
cd noksiy
cargo build --release
```

## Использование

### Запуск движка

```bash
cargo run --release
```

### UCI команды

```
uci          - идентификация движка
isready      - проверка готовности
position startpos moves e2e4 e7e5  - установка позиции
go depth 10  - поиск на глубину 10
quit         - выход
```

### Использование с GUI

Noksiy совместим с любыми шахматными интерфейсами, поддерживающими UCI:
- Arena
- Cutechess
- Lichess (через lichess-bot)
- ChessBase

## Архитектура

```
src/
├── main.rs        - точка входа
├── board.rs       - представление доски (bitboards)
├── movegen.rs     - генерация ходов
├── search.rs      - алгоритм поиска (alpha-beta, iterative deepening)
├── evaluation.rs  - оценка позиций
└── uci.rs         - UCI протокол
```

## Алгоритмы

### Поиск
- **Alpha-Beta Pruning** с отсечением неперспективных вариантов
- **Iterative Deepening** для оптимального использования времени
- **Quiescence Search** для тактической стабильности
- **Move Ordering** (MVV-LVA, killer moves, history heuristic)

### Оценка
- Материальный баланс
- Позиционные бонусы (Piece-Square Tables)
- Структура пешек (сдвоенные, проходные)
- Пара слонов
- Разделение миттельшпиля и эндшпиля

## Roadmap

- [ ] Transposition Table (хеш-таблица позиций)
- [ ] Null Move Pruning
- [ ] Late Move Reductions (LMR)
- [ ] Magic Bitboards для слайдеров
- [ ] Syzygy Tablebase
- [ ] Opening Book
- [ ] Multi-threading (Lazy SMP)
- [ ] Tunable evaluation параметры

## Разработка

### Сборка в debug режиме
```bash
cargo build
```

### Запуск тестов
```bash
cargo test
```

### Профилирование
```bash
cargo build --release
perf record ./target/release/noksiy
```

## Авторы

- **nesstyx** - основной разработчик
- **Claude** - AI ассистент (Anthropic)

## Лицензия

MIT License - см. LICENSE файл

## Контрибьюция

Pull requests приветствуются! Для больших изменений сначала откройте issue для обсуждения.

---

*Создано с ♥ и Rust*
