# Implementation Plan: Frontend-Backend Connection Fix

## Overview

This plan fixes the connection issues between the Tauri frontend and Python Sidecar backend by:
1. Enabling the Anki service registration
2. Adding parameter name normalization for camelCase/snake_case compatibility

## Tasks

- [ ] 1. Enable Anki Service Registration
  - [ ] 1.1 Uncomment AnkiService import and registration in `__main__.py`
    - File: `python/huge_sidecar/__main__.py`
    - Uncomment the AnkiService import line
    - Uncomment the `self.dispatcher.register('anki', AnkiService())` line
    - _Requirements: 2.1, 2.2, 2.3, 2.4_

- [ ] 2. Add Parameter Normalization to DocumentService
  - [ ] 2.1 Add parameter normalization in DocumentService.handle()
    - File: `python/huge_sidecar/services/document_service.py`
    - Add mapping for `inputPath` → `document_path`
    - Add mapping for `outputPath` → `output_path`
    - Apply normalization before processing the `format` method
    - _Requirements: 3.1, 3.2, 3.3_

- [ ] 3. Add Parameter Normalization to WebScraperService
  - [ ] 3.1 Update _parse_options() in WebScraperService
    - File: `python/huge_sidecar/services/web_scraper_service.py`
    - Add mapping for `downloadImages` → `save_images`
    - Add mapping for `outputDir` → `images_dir`
    - Add mappings for other camelCase options (waitUntil, waitForSelector, etc.)
    - _Requirements: 4.1, 4.2, 4.3_

- [ ] 4. Checkpoint - Verify Changes
  - Ensure all Python files have valid syntax
  - Run `cd python && python -c "from huge_sidecar import __main__"` to verify imports
  - Ask the user if questions arise

- [ ] 5. Write Unit Tests
  - [ ] 5.1 Write test for Anki service registration
    - Verify AnkiService is in dispatcher.list_services()
    - _Requirements: 2.1_
  
  - [ ] 5.2 Write test for DocumentService parameter normalization
    - **Property 2: DocumentService Parameter Normalization**
    - **Validates: Requirements 3.1, 3.2, 3.3**
  
  - [ ] 5.3 Write test for WebScraperService parameter normalization
    - **Property 3: WebScraperService Parameter Normalization**
    - **Validates: Requirements 4.1, 4.2, 4.3**

## Notes
- The translation service is already registered as `translate` (verified in current code)
- No changes needed to frontend code - all fixes are in Python backend
- Changes are minimal and focused only on connection issues
