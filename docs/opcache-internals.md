# OPcache Internals: Direct Opcode Access

Исследование возможности получения бинарных opcodes из OPcache для прямого выполнения в Rust.

## Структуры данных OPcache

### zend_persistent_script

Главная структура кэшированного скрипта:

```c
typedef struct _zend_persistent_script {
    zend_script    script;              // Скомпилированный скрипт
    zend_long      compiler_halt_offset;
    int            ping_auto_globals_mask;
    accel_time_t   timestamp;
    bool           corrupted;
    bool           is_phar;
    bool           empty;
    uint32_t       num_warnings;
    uint32_t       num_early_bindings;
    zend_error_info **warnings;
    zend_early_binding *early_bindings;

    void          *mem;                 // Указатель на память
    size_t         size;                // Размер в shared memory

    struct {
        time_t       last_used;
        zend_ulong   hits;              // Количество попаданий
        unsigned int memory_consumption;
        time_t       revalidate;
    } dynamic_members;
} zend_persistent_script;
```

### zend_op_array

Массив опкодов для выполнения:

```c
struct _zend_op_array {
    uint8_t type;
    zend_string *function_name;
    zend_class_entry *scope;
    uint32_t num_args;

    uint32_t last;              // Количество opcodes
    zend_op *opcodes;           // Массив опкодов

    zend_string **vars;         // Локальные переменные
    zval *literals;             // Литералы (строки, числа)

    zend_string *filename;
    uint32_t line_start;
    uint32_t line_end;

    HashTable *static_variables;
    // ...
};
```

### zend_op (один опкод)

```c
struct _zend_op {
    const void *handler;        // Указатель на обработчик
    znode_op op1;               // Первый операнд
    znode_op op2;               // Второй операнд
    znode_op result;            // Результат
    uint32_t extended_value;
    uint32_t lineno;
    uint8_t opcode;             // Тип операции (ZEND_ADD, ZEND_ECHO, etc.)
    uint8_t op1_type;
    uint8_t op2_type;
    uint8_t result_type;
};
```

## API для выполнения opcodes

### Стандартный путь

```c
// Компиляция файла
zend_op_array *op_array = zend_compile_file(&file_handle, ZEND_INCLUDE);

// Выполнение
zval return_value;
zend_execute(op_array, &return_value);
```

### Прямое выполнение через execute_ex

```c
// Низкоуровневое выполнение
zend_execute_data *execute_data = zend_vm_stack_push_call_frame(
    ZEND_CALL_TOP_CODE, op_array, 0, NULL
);
zend_init_code_execute_data(execute_data, op_array, &return_value);
execute_ex(execute_data);
```

## Проблемы прямого доступа

### 1. Нет публичного C API

OPcache не экспортирует функции для доступа к кэшированным скриптам:

```c
// Внутренняя функция (не экспортируется)
zend_persistent_script *zend_accel_find_script(
    zend_string *filename,
    int check_timestamp
);
```

### 2. Указатели привязаны к процессу

```
Shared Memory (OPcache)
┌─────────────────────────────────────┐
│ zend_persistent_script              │
│   ├── opcodes: 0x7f1234560000 ───┐  │  ← Абсолютный адрес
│   ├── literals: 0x7f1234561000   │  │
│   └── vars: 0x7f1234562000       │  │
└─────────────────────────────────────┘
                                   │
                                   ▼
                    Process A видит по этому адресу
                    Process B может mmap в другое место!
```

### 3. Runtime cache

```c
// Каждый запрос требует свой runtime cache
ZEND_MAP_PTR_DEF(void **, run_time_cache);  // Per-request данные
```

### 4. Зависимость от PHP версии

Opcodes несовместимы между:
- Разными версиями PHP (8.3 vs 8.4)
- Разными конфигурациями (ZTS vs NTS)
- Разными архитектурами (x86 vs ARM)

## Возможные подходы

### Подход 1: PHP Preloading

Предзагрузка скриптов при старте сервера:

```php
// preload.php
<?php
// Загружаем фреймворк один раз
require '/var/www/vendor/autoload.php';

// Предзагружаем классы
opcache_compile_file('/var/www/app/Kernel.php');
opcache_compile_file('/var/www/app/Controller.php');
```

```bash
# php.ini
opcache.preload=/var/www/preload.php
opcache.preload_user=www-data
```

**Преимущества:**
- +30-60% производительности для фреймворков
- Официально поддерживается
- Классы/функции загружаются один раз

**Ограничения:**
- Требует перезапуска PHP для обновления
- Не работает в Docker без специальной настройки

### Подход 2: Расширение для экспорта API

Создать PHP расширение которое экспортирует внутренние функции OPcache:

```c
// ext/tokio_opcache_bridge.c

PHP_FUNCTION(tokio_get_cached_script)
{
    char *filename;
    size_t filename_len;

    ZEND_PARSE_PARAMETERS_START(1, 1)
        Z_PARAM_STRING(filename, filename_len)
    ZEND_PARSE_PARAMETERS_END();

    // Получаем кэшированный скрипт
    zend_string *zfilename = zend_string_init(filename, filename_len, 0);
    zend_persistent_script *script = zend_accel_find_script(zfilename, 0);

    if (!script) {
        RETURN_NULL();
    }

    // Возвращаем информацию о скрипте
    array_init(return_value);
    add_assoc_long(return_value, "size", script->size);
    add_assoc_long(return_value, "hits", script->dynamic_members.hits);
    add_assoc_long(return_value, "opcodes_count", script->script.main_op_array.last);

    // Указатель на память (для FFI)
    add_assoc_long(return_value, "mem_ptr", (zend_long)script->mem);
}
```

**Проблема:** `zend_accel_find_script` - internal linkage, не экспортируется.

### Подход 3: Прямой доступ к shared memory

```rust
// Теоретический код - не работает напрямую

use std::fs::File;
use std::os::unix::io::AsRawFd;
use nix::sys::mman::{mmap, MapFlags, ProtFlags};

fn access_opcache_shm() -> Result<(), Error> {
    // OPcache использует mmap с фиксированным ключом
    // Найти через /proc/<pid>/maps

    // Проблема: структуры содержат указатели,
    // которые валидны только в контексте PHP процесса
}
```

**Проблема:** Указатели в структурах невалидны вне PHP.

### Подход 4: Кэширование op_array в расширении

```c
// В tokio_sapi расширении

static HashTable cached_scripts;  // Thread-local кэш

void tokio_cache_script(const char *filename, zend_op_array *op_array) {
    // Копируем op_array в thread-local storage
    // При следующем запросе используем копию
}

zend_op_array* tokio_get_cached_op_array(const char *filename) {
    // Возвращаем кэшированный op_array
    // Но нужно обновлять runtime cache каждый запрос!
}
```

**Это работает для immutable частей**, но:
- Runtime cache всё равно создаётся per-request
- Статические переменные per-request
- Сложная синхронизация

## Рекомендуемый подход для tokio_php

### Текущая архитектура (оптимальная)

```
Request → Worker Thread → php_request_startup() → execute → php_request_shutdown()
                              │
                              ▼
                    OPcache (shared memory)
                    ├── Cached op_arrays
                    ├── Interned strings
                    └── JIT compiled code
```

OPcache уже делает тяжёлую работу:
1. Кэширует скомпилированные скрипты
2. Разделяет память между потоками
3. JIT компилирует горячие пути

### Оптимизации без изменения архитектуры

```ini
; php.ini - Production settings

; Увеличить память для большего кэша
opcache.memory_consumption=256

; Отключить проверку timestamps (быстрее)
opcache.validate_timestamps=0

; Увеличить количество файлов
opcache.max_accelerated_files=20000

; JIT в режиме tracing
opcache.jit=tracing
opcache.jit_buffer_size=128M

; Preloading для фреймворков
opcache.preload=/var/www/preload.php
```

### Метрики для анализа

```php
<?php
$status = opcache_get_status(true);

// Эффективность кэша
$hit_rate = $status['opcache_statistics']['hits'] /
            ($status['opcache_statistics']['hits'] +
             $status['opcache_statistics']['misses']) * 100;

echo "Hit rate: {$hit_rate}%\n";
echo "Cached scripts: {$status['opcache_statistics']['num_cached_scripts']}\n";
echo "Memory used: " . round($status['memory_usage']['used_memory'] / 1024 / 1024, 2) . " MB\n";

// JIT статистика
if (isset($status['jit'])) {
    echo "JIT enabled: " . ($status['jit']['enabled'] ? 'Yes' : 'No') . "\n";
    echo "JIT buffer used: " . round($status['jit']['buffer_used'] / 1024 / 1024, 2) . " MB\n";
}
```

## Выводы

| Подход | Реализуемость | Выигрыш | Рекомендация |
|--------|---------------|---------|--------------|
| PHP Preloading | Высокая | +30-60% | Рекомендуется |
| Расширение-мост | Средняя | +5-10% | Сложно, риски |
| Прямой SHM доступ | Низкая | - | Не работает |
| Кэш в расширении | Средняя | +5% | Сложно |

**Рекомендация:** Использовать стандартные механизмы OPcache (preloading, правильная конфигурация, JIT) вместо попыток обойти его API.

## Источники

- [PHP OPcache Manual](https://www.php.net/manual/en/book.opcache.php)
- [How OPcache Works (Nikita Popov)](https://www.npopov.com/2021/10/13/How-opcache-works.html)
- [PHP RFC: Direct Execution Opcode](https://wiki.php.net/rfc/direct-execution-opcode) - отклонён
- [PHP Preloading](https://www.php.net/manual/en/opcache.preloading.php)
- [php-src/ext/opcache](https://github.com/php/php-src/tree/master/ext/opcache)
