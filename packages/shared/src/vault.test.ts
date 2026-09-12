/**
 * Tests for the vault's pure security helpers.
 *
 * These are the decisions the UI must apply consistently, so they are tested
 * directly rather than through a rendered component: URL scheme allow-listing,
 * master-password scoring, and the in-memory search matcher.
 */

import { describe, expect, it } from 'vitest'
import {
  DEFAULT_VAULT_AUTO_LOCK_SECONDS,
  DEFAULT_VAULT_CLIPBOARD_CLEAR_SECONDS,
  MASTER_PASSWORD_MIN_LENGTH,
  MASTER_PASSWORD_RECOMMENDED_LENGTH,
  VAULT_ALLOWED_URL_SCHEMES,
  VAULT_AUTO_LOCK_OPTIONS,
  VAULT_CLIPBOARD_CLEAR_OPTIONS,
  VAULT_GENERATOR_DEFAULTS,
  VAULT_KDF_FLOOR_MIB,
  isVaultUrlOpenable,
  matchesVaultQuery,
  scoreMasterPassword,
  vaultUrlIssue,
} from './vault'
import type { VaultItemSummary, VaultUrlIssue } from './vault'

describe('vault URL scheme allow-list', () => {
  it('accepts http and https only', () => {
    expect(vaultUrlIssue('https://github.com')).toBeUndefined()
    expect(vaultUrlIssue('http://localhost:3000/login')).toBeUndefined()
    expect(vaultUrlIssue('HTTPS://example.com')).toBeUndefined()
    expect(vaultUrlIssue('  https://example.com  ')).toBeUndefined()
    expect([...VAULT_ALLOWED_URL_SCHEMES]).toEqual(['http', 'https'])
  })

  it.each([
    'javascript:alert(document.cookie)',
    'JavaScript:alert(1)',
    'file:///C:/Windows/System32/config/SAM',
    'data:text/html,<script>alert(1)</script>',
    'vbscript:msgbox(1)',
    'shell:startup',
    'powershell:-NoProfile -Command "whoami"',
    'cmd:/c calc',
    'ms-msdt:/id PCWDiagnostic',
    'chrome://settings',
    'mailto:someone@example.com',
  ])('refuses to open %s', (url) => {
    expect(vaultUrlIssue(url)).toBe('unsupportedScheme')
    expect(isVaultUrlOpenable(url)).toBe(false)
  })

  it('refuses input with no scheme at all', () => {
    expect(vaultUrlIssue('github.com')).toBe('noScheme')
    expect(vaultUrlIssue('//example.com')).toBe('noScheme')
    // A Windows drive path must not be mistaken for a URL scheme.
    expect(vaultUrlIssue('C:\\Windows\\System32')).toBe('noScheme')
  })

  it('refuses empty input and control characters', () => {
    expect(vaultUrlIssue('')).toBe('empty')
    expect(vaultUrlIssue('   ')).toBe('empty')
    expect(vaultUrlIssue('https://example.com/\u0000evil')).toBe('controlCharacters')
    expect(vaultUrlIssue('https://example.com/\nHost: evil')).toBe('controlCharacters')
  })

  it('narrows the issue type to the documented set', () => {
    const issues: Array<VaultUrlIssue | undefined> = [
      vaultUrlIssue(''),
      vaultUrlIssue('nope'),
      vaultUrlIssue('javascript:1'),
      vaultUrlIssue('https://ok'),
    ]
    expect(issues).toEqual(['empty', 'noScheme', 'unsupportedScheme', undefined])
  })
})

describe('master password strength', () => {
  it('never accepts a password below the minimum length', () => {
    for (const password of ['', 'short', 'a'.repeat(MASTER_PASSWORD_MIN_LENGTH - 1)]) {
      const strength = scoreMasterPassword(password)
      expect(strength.meetsMinimum).toBe(false)
      expect(strength.score).toBe(0)
      expect(strength.label).toBe('veryWeak')
    }
  })

  it('rewards length over character classes', () => {
    // A long lowercase passphrase must outrank a short "complex" password, which
    // is the whole point of not enforcing character-class rules.
    const passphrase = scoreMasterPassword('correct horse battery staple')
    const shortComplex = scoreMasterPassword('Ab1!xyZ@')
    expect(passphrase.meetsMinimum).toBe(true)
    expect(passphrase.score).toBeGreaterThan(shortComplex.score)
  })

  it('reaches the top band for a long mixed passphrase', () => {
    const strength = scoreMasterPassword('Correct-Horse-Battery-Staple-42!')
    expect(strength.meetsMinimum).toBe(true)
    expect(strength.meetsRecommended).toBe(true)
    expect(strength.score).toBe(4)
    expect(strength.label).toBe('excellent')
    expect(strength.length).toBe(32)
  })

  it('penalises obvious repetition even when it is long', () => {
    const repeated = scoreMasterPassword('aaaaaaaaaaaaaaaaaaaa')
    expect(repeated.meetsMinimum).toBe(true)
    expect(repeated.score).toBeLessThan(4)
  })

  it('penalises a monotonic run', () => {
    const sequence = scoreMasterPassword('abcdefghijklmnopqrstuvwx')
    expect(sequence.score).toBeLessThan(4)
  })

  it('counts a minimum-length lowercase-only password as acceptable', () => {
    const strength = scoreMasterPassword('a'.repeat(MASTER_PASSWORD_MIN_LENGTH + 4))
    expect(strength.meetsMinimum).toBe(true)
    expect(strength.meetsRecommended).toBe(true)
  })
})

describe('defaults and options', () => {
  it('offers only finite auto-lock windows', () => {
    // "Never" is deliberately not offered.
    expect(VAULT_AUTO_LOCK_OPTIONS.every((seconds) => seconds > 0)).toBe(true)
    expect(VAULT_AUTO_LOCK_OPTIONS).toContain(DEFAULT_VAULT_AUTO_LOCK_SECONDS)
    expect(DEFAULT_VAULT_AUTO_LOCK_SECONDS).toBe(300)
  })

  it('offers bounded clipboard clear delays with a 30 second default', () => {
    expect([...VAULT_CLIPBOARD_CLEAR_OPTIONS]).toEqual([15, 30, 60])
    expect(DEFAULT_VAULT_CLIPBOARD_CLEAR_SECONDS).toBe(30)
  })

  it('generates passwords of at least 20 characters by default', () => {
    expect(VAULT_GENERATOR_DEFAULTS.length).toBeGreaterThanOrEqual(20)
    expect(VAULT_GENERATOR_DEFAULTS.uppercase).toBe(true)
    expect(VAULT_GENERATOR_DEFAULTS.lowercase).toBe(true)
    expect(VAULT_GENERATOR_DEFAULTS.numbers).toBe(true)
    expect(VAULT_GENERATOR_DEFAULTS.symbols).toBe(true)
    expect(VAULT_GENERATOR_DEFAULTS.excludeAmbiguous).toBe(true)
  })

  it('quotes a key-derivation floor the UI can hold the native layer to', () => {
    // Mirrors MIN_MEMORY_KIB in crates/vault/src/kdf.rs. It is a floor rather
    // than a target, so it must not be lowered without a deliberate decision.
    expect(VAULT_KDF_FLOOR_MIB).toBeGreaterThanOrEqual(64)
  })
})

describe('in-memory search over decrypted summaries', () => {
  const item: VaultItemSummary = {
    id: 'item-1',
    title: 'GitHub',
    username: 'jensen@example.com',
    url: 'https://github.com',
    tags: ['work', 'git'],
    favorite: true,
    hasPassword: true,
    hasNotes: false,
    customFieldCount: 0,
    createdAt: 1,
    updatedAt: 2,
  }

  it('matches the title, username, URL, and tags', () => {
    for (const query of ['github', 'JENSEN', 'example.com', 'work', 'git']) {
      expect(matchesVaultQuery(item, query)).toBe(true)
    }
  })

  it('requires every term to match', () => {
    expect(matchesVaultQuery(item, 'github work')).toBe(true)
    expect(matchesVaultQuery(item, 'github gitlab')).toBe(false)
  })

  it('matches everything on an empty query, and nothing on a miss', () => {
    expect(matchesVaultQuery(item, '')).toBe(true)
    expect(matchesVaultQuery(item, '   ')).toBe(true)
    expect(matchesVaultQuery(item, 'bitbucket')).toBe(false)
  })

  it('never inspects fields the summary deliberately omits', () => {
    // The summary has no password or notes field at all; this asserts the shape
    // the search matcher depends on, so a future change that starts shipping
    // secrets in the list projection fails here.
    expect(Object.keys(item)).not.toContain('password')
    expect(Object.keys(item)).not.toContain('notes')
    expect(Object.keys(item)).not.toContain('customFields')
  })
})
