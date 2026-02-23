# Requirements Document

## Introduction

This document specifies the requirements for fixing the connection issues between the Tauri frontend and Python Sidecar backend in the HuGeScreenshot-tauri project. The backend services are implemented but have naming mismatches and missing registrations that prevent the frontend from correctly invoking them.

## Glossary

- **Sidecar**: The Python backend process that communicates with the Tauri frontend via stdin/stdout JSON messages
- **Service**: A Python class that handles specific functionality (OCR, translation, Anki, etc.)
- **Dispatcher**: The component that routes incoming requests to the appropriate service based on service name
- **Frontend**: The Vue 3 + TypeScript application running in the Tauri webview
- **camelCase**: Naming convention used in JavaScript/TypeScript (e.g., `inputPath`)
- **snake_case**: Naming convention used in Python (e.g., `input_path`)

## Requirements

### Requirement 1: Translation Service Name Alignment

**User Story:** As a user, I want to use the translation feature, so that I can translate text between languages.

#### Acceptance Criteria

1. WHEN the frontend calls the translation service with `service: 'translate'` THEN the Sidecar SHALL route the request to the TranslationService
2. THE Sidecar SHALL register the TranslationService under the name `translate` (not `translation`)

### Requirement 2: Anki Service Registration

**User Story:** As a user, I want to create Anki flashcards from screenshots, so that I can study vocabulary and concepts.

#### Acceptance Criteria

1. WHEN the Sidecar starts THEN the Sidecar SHALL register the AnkiService
2. WHEN the frontend calls `service: 'anki'` with method `add_card` THEN the Sidecar SHALL route the request to AnkiService.add_card
3. WHEN the frontend calls `service: 'anki'` with method `check_connection` THEN the Sidecar SHALL route the request to AnkiService.check_connection
4. WHEN the frontend calls `service: 'anki'` with method `get_decks` THEN the Sidecar SHALL route the request to AnkiService.get_decks

### Requirement 3: Document Service Parameter Compatibility

**User Story:** As a user, I want to format government documents, so that they comply with GB/T 9704-2012 standards.

#### Acceptance Criteria

1. WHEN the frontend calls document format with `inputPath` parameter THEN the DocumentService SHALL accept it as equivalent to `document_path`
2. WHEN the frontend calls document format with `outputPath` parameter THEN the DocumentService SHALL accept it as equivalent to `output_path`
3. THE DocumentService SHALL support both camelCase and snake_case parameter names for backward compatibility

### Requirement 4: Web Scraper Parameter Compatibility

**User Story:** As a user, I want to scrape web pages and save them as Markdown, so that I can archive web content.

#### Acceptance Criteria

1. WHEN the frontend calls web scrape with `downloadImages` parameter THEN the WebScraperService SHALL accept it as equivalent to `save_images`
2. WHEN the frontend calls web scrape with `outputDir` parameter THEN the WebScraperService SHALL accept it as equivalent to `images_dir`
3. THE WebScraperService SHALL support both camelCase and snake_case parameter names for backward compatibility
