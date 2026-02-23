/**
 * Property-Based Tests for Color Preview Consistency
 *
 * Feature: settings-enhancement
 *
 * This file tests the color preview behavior for the ColorPicker component:
 * - Property 8: Color Preview Consistency
 *
 * **Validates: Requirements 4.6**
 */

import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import * as fc from 'fast-check'
import ColorPicker from '../../controls/ColorPicker.vue'

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Generates a valid hex color string from RGB components.
 *
 * @param r - Red component (0-255)
 * @param g - Green component (0-255)
 * @param b - Blue component (0-255)
 * @returns Hex color string in format "#RRGGBB"
 */
function rgbToHex(r: number, g: number, b: number): string {
  const toHex = (n: number): string => {
    const hex = Math.round(n).toString(16).toUpperCase()
    return hex.length === 1 ? '0' + hex : hex
  }
  return `#${toHex(r)}${toHex(g)}${toHex(b)}`
}

/**
 * Normalizes a hex color string to uppercase format.
 *
 * @param hex - Hex color string (e.g., "#ff0000" or "#FF0000")
 * @returns Normalized hex color string in uppercase (e.g., "#FF0000")
 */
function normalizeHexColor(hex: string): string {
  return hex.toUpperCase()
}

/**
 * Parses a CSS color value (hex or rgb) to a normalized hex string.
 * Handles both "#RRGGBB" format and "rgb(r, g, b)" format.
 *
 * @param cssColor - CSS color value
 * @returns Normalized hex color string
 */
function parseCssColorToHex(cssColor: string): string {
  // If already hex format
  if (cssColor.startsWith('#')) {
    return normalizeHexColor(cssColor)
  }

  // If rgb format: "rgb(r, g, b)"
  const rgbMatch = cssColor.match(/rgb\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*\)/)
  if (rgbMatch) {
    const r = parseInt(rgbMatch[1], 10)
    const g = parseInt(rgbMatch[2], 10)
    const b = parseInt(rgbMatch[3], 10)
    return rgbToHex(r, g, b)
  }

  // Return as-is if format not recognized (normalized)
  return normalizeHexColor(cssColor)
}

// ============================================================================
// Custom Arbitraries
// ============================================================================

/**
 * Arbitrary for generating valid hex color strings.
 *
 * Generates colors by creating random RGB values (0-255) and converting to hex.
 * This ensures all generated colors are valid hex colors.
 */
const hexColorArbitrary = fc
  .tuple(
    fc.integer({ min: 0, max: 255 }), // Red
    fc.integer({ min: 0, max: 255 }), // Green
    fc.integer({ min: 0, max: 255 }) // Blue
  )
  .map(([r, g, b]) => rgbToHex(r, g, b))

// ============================================================================
// Property 8: Color Preview Consistency
// ============================================================================

describe('Feature: settings-enhancement, Property 8: Color Preview Consistency', () => {
  /**
   * Property 8: Color Preview Consistency
   *
   * For any valid hex color value selected in the color picker,
   * the preview element's background color SHALL match the selected color.
   *
   * **Validates: Requirements 4.6**
   */

  it('should display preview background color matching the input color for any valid hex color', () => {
    fc.assert(
      fc.property(hexColorArbitrary, (hexColor: string) => {
        // Mount the ColorPicker component with the generated color
        const wrapper = mount(ColorPicker, {
          props: {
            modelValue: hexColor,
          },
        })

        // Find the preview element
        const previewElement = wrapper.find('.color-preview')
        expect(previewElement.exists()).toBe(true)

        // Get the background color style
        const backgroundStyle = previewElement.attributes('style')
        expect(backgroundStyle).toBeDefined()

        // Extract the background-color value from the style attribute
        // The style is in format: "background-color: #RRGGBB;" or "background-color: rgb(r, g, b);"
        const bgColorMatch = backgroundStyle?.match(/background-color:\s*([^;]+)/)
        expect(bgColorMatch).not.toBeNull()

        const actualBgColor = bgColorMatch![1].trim()

        // Parse and normalize both colors for comparison
        const normalizedActual = parseCssColorToHex(actualBgColor)
        const normalizedExpected = normalizeHexColor(hexColor)

        // Property: Preview background color matches the input color
        expect(normalizedActual).toBe(normalizedExpected)

        // Cleanup
        wrapper.unmount()
      }),
      {
        numRuns: 100, // Minimum 100 iterations as per spec
        verbose: true,
      }
    )
  })

  it('should update preview background color when modelValue changes', async () => {
    // Use asyncProperty for async operations with Vue reactivity
    await fc.assert(
      fc.asyncProperty(
        hexColorArbitrary,
        hexColorArbitrary,
        async (initialColor: string, newColor: string) => {
          // Mount with initial color
          const wrapper = mount(ColorPicker, {
            props: {
              modelValue: initialColor,
            },
          })

          // Verify initial color
          const previewElement = wrapper.find('.color-preview')
          let backgroundStyle = previewElement.attributes('style')
          let bgColorMatch = backgroundStyle?.match(/background-color:\s*([^;]+)/)
          let actualBgColor = parseCssColorToHex(bgColorMatch![1].trim())
          expect(actualBgColor).toBe(normalizeHexColor(initialColor))

          // Update to new color and wait for Vue to update the DOM
          await wrapper.setProps({ modelValue: newColor })

          // Verify new color is reflected in preview
          backgroundStyle = previewElement.attributes('style')
          bgColorMatch = backgroundStyle?.match(/background-color:\s*([^;]+)/)
          actualBgColor = parseCssColorToHex(bgColorMatch![1].trim())

          // Property: Preview updates to match new color
          expect(actualBgColor).toBe(normalizeHexColor(newColor))

          // Cleanup
          wrapper.unmount()
        }
      ),
      {
        numRuns: 100,
        verbose: true,
      }
    )
  })

  it('should handle edge case colors correctly', () => {
    // Test specific edge case colors
    const edgeCaseColors = [
      '#000000', // Black
      '#FFFFFF', // White
      '#FF0000', // Pure Red
      '#00FF00', // Pure Green
      '#0000FF', // Pure Blue
      '#FFFF00', // Yellow
      '#FF00FF', // Magenta
      '#00FFFF', // Cyan
      '#808080', // Gray
      '#123456', // Random
    ]

    for (const color of edgeCaseColors) {
      const wrapper = mount(ColorPicker, {
        props: {
          modelValue: color,
        },
      })

      const previewElement = wrapper.find('.color-preview')
      const backgroundStyle = previewElement.attributes('style')
      const bgColorMatch = backgroundStyle?.match(/background-color:\s*([^;]+)/)
      const actualBgColor = parseCssColorToHex(bgColorMatch![1].trim())

      // Property: Preview matches input for edge case colors
      expect(actualBgColor).toBe(normalizeHexColor(color))

      wrapper.unmount()
    }
  })

  it('should have preview element with correct title attribute showing the color value', () => {
    fc.assert(
      fc.property(hexColorArbitrary, (hexColor: string) => {
        const wrapper = mount(ColorPicker, {
          props: {
            modelValue: hexColor,
          },
        })

        const previewElement = wrapper.find('.color-preview')

        // The title attribute should show the color value for accessibility
        const titleAttr = previewElement.attributes('title')
        expect(titleAttr).toBe(hexColor)

        wrapper.unmount()
      }),
      {
        numRuns: 100,
        verbose: true,
      }
    )
  })

  it('should handle lowercase hex colors by displaying them correctly', () => {
    fc.assert(
      fc.property(hexColorArbitrary, (hexColor: string) => {
        // Convert to lowercase to test case handling
        const lowercaseColor = hexColor.toLowerCase()

        const wrapper = mount(ColorPicker, {
          props: {
            modelValue: lowercaseColor,
          },
        })

        const previewElement = wrapper.find('.color-preview')
        const backgroundStyle = previewElement.attributes('style')
        const bgColorMatch = backgroundStyle?.match(/background-color:\s*([^;]+)/)
        const actualBgColor = parseCssColorToHex(bgColorMatch![1].trim())

        // Property: Preview matches input regardless of case
        expect(actualBgColor).toBe(normalizeHexColor(lowercaseColor))

        wrapper.unmount()
      }),
      {
        numRuns: 100,
        verbose: true,
      }
    )
  })
})

// ============================================================================
// Color Generation Properties
// ============================================================================

describe('Feature: settings-enhancement, Color Generation Properties', () => {
  /**
   * Additional properties to verify the color generation and parsing utilities
   */

  it('should generate valid hex colors from any RGB values', () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 0, max: 255 }),
        fc.integer({ min: 0, max: 255 }),
        fc.integer({ min: 0, max: 255 }),
        (r: number, g: number, b: number) => {
          const hex = rgbToHex(r, g, b)

          // Property: Generated hex is valid format
          expect(hex).toMatch(/^#[0-9A-F]{6}$/)

          // Property: Hex is exactly 7 characters
          expect(hex.length).toBe(7)

          // Property: Starts with #
          expect(hex[0]).toBe('#')
        }
      ),
      {
        numRuns: 100,
        verbose: true,
      }
    )
  })

  it('should round-trip RGB to hex and back correctly', () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 0, max: 255 }),
        fc.integer({ min: 0, max: 255 }),
        fc.integer({ min: 0, max: 255 }),
        (r: number, g: number, b: number) => {
          const hex = rgbToHex(r, g, b)

          // Parse hex back to RGB
          const parsedR = parseInt(hex.slice(1, 3), 16)
          const parsedG = parseInt(hex.slice(3, 5), 16)
          const parsedB = parseInt(hex.slice(5, 7), 16)

          // Property: Round-trip preserves values
          expect(parsedR).toBe(r)
          expect(parsedG).toBe(g)
          expect(parsedB).toBe(b)
        }
      ),
      {
        numRuns: 100,
        verbose: true,
      }
    )
  })

  it('should normalize hex colors to uppercase consistently', () => {
    fc.assert(
      fc.property(hexColorArbitrary, (hexColor: string) => {
        const lowercase = hexColor.toLowerCase()
        const uppercase = hexColor.toUpperCase()

        // Property: Normalization produces consistent results
        expect(normalizeHexColor(lowercase)).toBe(normalizeHexColor(uppercase))
        expect(normalizeHexColor(hexColor)).toBe(uppercase)
      }),
      {
        numRuns: 100,
        verbose: true,
      }
    )
  })
})
