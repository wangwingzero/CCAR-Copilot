# Design Document: Frontend-Backend Connection Fix

## Overview

This design addresses the connection issues between the Tauri frontend (Vue 3 + TypeScript) and the Python Sidecar backend. The issues stem from:

1. Service name mismatch (frontend calls `translate`, backend registers `translation`)
2. Anki service not registered (commented out in `__main__.py`)
3. Parameter naming convention differences (camelCase vs snake_case)

The fix involves minimal changes to the Python backend to align with frontend expectations while maintaining backward compatibility.

## Architecture

The existing architecture follows a clean separation:

```
┌─────────────────────────────────────────────────────────────┐
│                    Tauri Frontend (Vue 3)                    │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ sidecar.ts  │  │ useAnki.ts  │  │ useDocumentFormatter│  │
│  │ (store)     │  │             │  │                     │  │
│  └──────┬──────┘  └──────┬──────┘  └──────────┬──────────┘  │
│         │                │                     │             │
│         └────────────────┼─────────────────────┘             │
│                          │                                   │
│                    invoke('call_sidecar', {...})             │
└──────────────────────────┼───────────────────────────────────┘
                           │ JSON via stdin/stdout
┌──────────────────────────┼───────────────────────────────────┐
│                    Python Sidecar                            │
│                          │                                   │
│  ┌───────────────────────▼───────────────────────────────┐  │
│  │              ServiceDispatcher                         │  │
│  │  ┌─────────────────────────────────────────────────┐  │  │
│  │  │ services = {                                     │  │  │
│  │  │   'ocr': OcrService,                            │  │  │
│  │  │   'translate': TranslationService,  ← FIX #1    │  │  │
│  │  │   'anki': AnkiService,              ← FIX #2    │  │  │
│  │  │   'document': DocumentService,                  │  │  │
│  │  │   'web': WebScraperService,                     │  │  │
│  │  │   ...                                           │  │  │
│  │  │ }                                               │  │  │
│  │  └─────────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

## Components and Interfaces

### Component 1: Service Registration (`__main__.py`)

**Current State:**
```python
# Translation service registered as 'translate' (already correct in current code)
self.dispatcher.register("translate", TranslationService())

# Anki service commented out
# from .services.anki_service import AnkiService
# self.dispatcher.register('anki', AnkiService())
```

**Target State:**
```python
# Translation service (already correct)
self.dispatcher.register("translate", TranslationService())

# Anki service (uncomment and register)
from .services.anki_service import AnkiService
self.dispatcher.register('anki', AnkiService())
```

### Component 2: Parameter Normalization Utility

Create a utility function to normalize parameter names from camelCase to snake_case:

```python
def normalize_params(params: dict[str, Any], mappings: dict[str, str]) -> dict[str, Any]:
    """
    Normalize parameter names for frontend compatibility.
    
    Args:
        params: Original parameters from frontend
        mappings: Dict mapping camelCase names to snake_case names
                  e.g., {'inputPath': 'document_path', 'outputPath': 'output_path'}
    
    Returns:
        Normalized parameters with snake_case names
    """
    normalized = dict(params)
    for camel_name, snake_name in mappings.items():
        if camel_name in normalized and snake_name not in normalized:
            normalized[snake_name] = normalized.pop(camel_name)
    return normalized
```

### Component 3: DocumentService Parameter Handling

**Location:** `services/document_service.py`

**Changes to `handle()` method:**

```python
async def handle(self, method: str, params: dict[str, Any]) -> Any:
    # Normalize camelCase parameters from frontend
    param_mappings = {
        'inputPath': 'document_path',
        'outputPath': 'output_path',
    }
    params = normalize_params(params, param_mappings)
    
    if method == "format":
        return await self.format_document(
            params.get("document_path", ""),
            params.get("output_path"),
            params.get("options", {})
        )
    # ... rest of methods
```

### Component 4: WebScraperService Parameter Handling

**Location:** `services/web_scraper_service.py`

**Changes to `_parse_options()` method:**

```python
def _parse_options(self, options_dict: dict[str, Any]) -> ScrapeOptions:
    """Parse scrape options with camelCase compatibility."""
    # Normalize camelCase parameters from frontend
    param_mappings = {
        'downloadImages': 'save_images',
        'outputDir': 'images_dir',
        'waitUntil': 'wait_until',
        'waitForSelector': 'wait_for_selector',
        'contentSelector': 'content_selector',
        'rateLimit': 'rate_limit',
        'respectRobotsTxt': 'respect_robots_txt',
        'userAgent': 'user_agent',
        'scrollToBottom': 'scroll_to_bottom',
    }
    
    normalized = {}
    for key, value in options_dict.items():
        snake_key = param_mappings.get(key, key)
        normalized[snake_key] = value
    
    options = ScrapeOptions()
    for key, value in normalized.items():
        if hasattr(options, key):
            setattr(options, key, value)
    
    return options
```

## Data Models

No new data models are required. The existing data models remain unchanged:

- `SidecarRequest`: `{id, service, method, params}`
- `SidecarResponse`: `{id, success, result?, error?}`

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system—essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Property 1: Translation Service Routing

*For any* valid translation request with `service: 'translate'`, the Sidecar dispatcher SHALL route it to the TranslationService and return a valid translation response.

**Validates: Requirements 1.1, 1.2**

### Property 2: DocumentService Parameter Normalization

*For any* document format request, calling with camelCase parameters (`inputPath`, `outputPath`) SHALL produce the same result as calling with snake_case parameters (`document_path`, `output_path`).

**Validates: Requirements 3.1, 3.2, 3.3**

### Property 3: WebScraperService Parameter Normalization

*For any* web scrape request options, calling with camelCase parameters (`downloadImages`, `outputDir`) SHALL produce the same ScrapeOptions as calling with snake_case parameters (`save_images`, `images_dir`).

**Validates: Requirements 4.1, 4.2, 4.3**

## Error Handling

### Service Not Found

When a frontend request specifies an unregistered service name:
- The dispatcher returns `{"success": false, "error": "[SERVICE_NOT_FOUND] Unknown service: {name}"}`
- No exception is raised; the error is returned as a JSON response

### Parameter Validation

When required parameters are missing after normalization:
- Each service validates its required parameters
- Returns descriptive error messages (e.g., "Document path is required")

### Anki Connection Errors

When AnkiConnect is not available:
- `check_connection` returns `{"connected": false, "error": "...", "code": "CONNECTION_ERROR"}`
- `add_card` and `get_decks` raise `AnkiError` with code `CONNECTION_ERROR`

## Testing Strategy

### Unit Tests

1. **Service Registration Test**: Verify all expected services are registered after Sidecar startup
2. **Parameter Normalization Tests**: Test the `normalize_params` utility function with various inputs
3. **Anki Service Methods**: Test each Anki method is accessible and returns expected structure

### Property-Based Tests

Property tests should use a property-based testing library (e.g., `hypothesis` for Python) with minimum 100 iterations per property.

1. **Property 1 Test**: Generate random translation requests and verify routing
   - Tag: **Feature: frontend-backend-connection-fix, Property 1: Translation Service Routing**

2. **Property 2 Test**: Generate random document paths and verify camelCase/snake_case equivalence
   - Tag: **Feature: frontend-backend-connection-fix, Property 2: DocumentService Parameter Normalization**

3. **Property 3 Test**: Generate random scrape options and verify camelCase/snake_case equivalence
   - Tag: **Feature: frontend-backend-connection-fix, Property 3: WebScraperService Parameter Normalization**

### Integration Tests

1. **End-to-End Translation**: Frontend → Rust → Python → TranslationService → Response
2. **End-to-End Anki**: Frontend → Rust → Python → AnkiService → Response (requires Anki running)
3. **End-to-End Document Format**: Frontend → Rust → Python → DocumentService → Response

