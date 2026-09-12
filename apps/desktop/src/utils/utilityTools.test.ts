import { describe, expect, it } from 'vitest'
import {
  decodeBase64,
  decodeJwt,
  diagnoseJson,
  diffJson,
  diffText,
  decodeUrlComponent,
  encodeBase64,
  encodeUrlComponent,
  formatJson,
  formatTimestamp,
  hmacText,
  hashText,
  inspectJwt,
  buildSqlIn,
  fillSqlParameters,
  formatSql,
  minifySql,
  previewSqlDdl,
  restoreMyBatisSql,
  testRegex,
} from './utilityTools'

describe('JSON helpers', () => {
  it('formats and minifies JSON without changing its value', () => {
    const input = '{"name":"Dev Workbench","tools":["json","regex"]}'
    expect(formatJson(input)).toBe('{\n  "name": "Dev Workbench",\n  "tools": [\n    "json",\n    "regex"\n  ]\n}')
    expect(formatJson(input, true)).toBe('{"name":"Dev Workbench","tools":["json","regex"]}')
  })

  it('surfaces malformed JSON to the caller', () => {
    expect(() => formatJson('{"name":}')).toThrow()
  })

  it('reports a useful line and column for malformed JSON', () => {
    expect(diagnoseJson('{\n  "name":\n}')).toMatchObject({ line: 3, column: 1 })
  })

  it('creates a line-level diff', () => {
    expect(diffText('one\ntwo', 'one\nthree')).toEqual([
      { type: 'unchanged', value: 'one', line: 1 },
      { type: 'removed', value: 'two', line: 2 },
      { type: 'added', value: 'three', line: 2 },
    ])
  })

  it('compares JSON by path', () => {
    expect(diffJson('{"user":{"name":"Ada"}}', '{"user":{"name":"Linus","role":"maintainer"}}')).toEqual([
      { type: 'changed', path: '$.user.name', before: 'Ada', after: 'Linus' },
      { type: 'added', path: '$.user.role', after: 'maintainer' },
    ])
  })
})

describe('base64 helpers', () => {
  it('round-trips unicode text', () => {
    const encoded = encodeBase64('Dev Workbench 本地工具')
    expect(decodeBase64(encoded)).toBe('Dev Workbench 本地工具')
  })

  it('rejects malformed base64', () => {
    expect(() => decodeBase64('not base64!')).toThrow()
  })
})

describe('URL helpers', () => {
  it('round-trips query text without contacting a remote service', () => {
    const value = 'name=Dev Workbench&scope=本地工具'
    expect(decodeUrlComponent(encodeUrlComponent(value))).toBe(value)
    expect(encodeUrlComponent(value)).toContain('%20')
  })

  it('rejects malformed percent escapes', () => {
    expect(() => decodeUrlComponent('%E0%A4%A')).toThrow()
  })
})

describe('decodeJwt', () => {
  it('decodes header and payload without pretending to verify the signature', () => {
    const token = [
      { alg: 'HS256', typ: 'JWT' },
      { sub: 'developer', scope: ['read'] },
      'signature-is-not-verified',
    ].map((part, index) => index === 2 ? part : toBase64Url(JSON.stringify(part))).join('.')

    expect(decodeJwt(token)).toEqual({
      header: { alg: 'HS256', typ: 'JWT' },
      payload: { sub: 'developer', scope: ['read'] },
    })
  })

  it('rejects tokens that do not contain JSON header and payload', () => {
    expect(() => decodeJwt('one.part')).toThrow()
    expect(() => decodeJwt('e30.invalid.signature')).toThrow()
  })

  it('exposes the signature as opaque metadata', () => {
    const token = [
      { alg: 'HS256' },
      { exp: 2000000000, sub: 'developer' },
      'opaque-signature',
    ].map((part, index) => index === 2 ? part : toBase64Url(JSON.stringify(part))).join('.')
    expect(inspectJwt(token)).toMatchObject({ algorithm: 'HS256', signature: 'opaque-signature', claims: { sub: 'developer' } })
  })
})

describe('formatTimestamp', () => {
  it('detects seconds and milliseconds', () => {
    expect(formatTimestamp('1700000000')).toMatchObject({
      unit: 'seconds',
      milliseconds: 1700000000000,
      iso: '2023-11-14T22:13:20.000Z',
    })
    expect(formatTimestamp('1700000000000')).toMatchObject({
      unit: 'milliseconds',
      milliseconds: 1700000000000,
    })
  })

  it('accepts ISO date input and rejects invalid dates', () => {
    expect(formatTimestamp('2024-01-02T03:04:05Z')).toMatchObject({
      unit: 'date',
      seconds: 1704164645,
    })
    expect(() => formatTimestamp('not a timestamp')).toThrow()
  })
})

describe('testRegex', () => {
  it('returns match text, indexes, and capture groups', () => {
    expect(testRegex('(dev)[ -](\\w+)', 'gi', 'Dev Workbench dev tools')).toEqual({
      matches: [
        { value: 'Dev Workbench', index: 0, groups: ['Dev', 'Workbench'] },
        { value: 'dev tools', index: 14, groups: ['dev', 'tools'] },
      ],
      truncated: false,
    })
  })

  it('handles zero-length matches without looping forever', () => {
    expect(testRegex('^|$', 'g', 'abc').matches).toHaveLength(2)
  })
})

describe('hashText', () => {
  it('returns a lowercase hexadecimal digest', async () => {
    await expect(hashText('abc', 'SHA-256')).resolves.toBe(
      'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
    )
  })

  it('calculates HMAC without exposing the secret in the result contract', async () => {
    await expect(hmacText('abc', 'secret', 'HMAC-SHA256')).resolves.toHaveLength(64)
  })
})

describe('SQL helpers', () => {
  it('formats common SQL clauses and operators', () => {
    expect(formatSql('select * from sys_user where id=1 and status=1')).toBe(
      'SELECT *\nFROM sys_user\nWHERE id = 1\n  AND status = 1',
    )
  })

  it('minifies SQL without changing quoted values', () => {
    expect(minifySql("SELECT *\nFROM users\nWHERE name = 'A  B' AND id = 1")).toBe(
      "SELECT * FROM users WHERE name='A  B' AND id=1",
    )
  })

  it('builds a de-duplicated IN expression for strings and numbers', () => {
    expect(buildSqlIn('1001\n1002\n1002', 'string')).toBe("IN ('1001', '1002')")
    expect(buildSqlIn('1\n2\n1', 'number')).toBe('IN (1, 2)')
    expect(() => buildSqlIn('1\nnope', 'number')).toThrow('line 2')
  })

  it('fills question-mark parameters without replacing quoted question marks', () => {
    expect(fillSqlParameters("SELECT * FROM users WHERE note = '?' AND id = ?", '1001(Long)')).toBe("SELECT * FROM users WHERE note = '?' AND id = 1001")
  })

  it('previews CREATE TABLE columns without executing the DDL', () => {
    expect(previewSqlDdl('CREATE TABLE sys_user (id BIGINT PRIMARY KEY, name VARCHAR(64) NOT NULL, PRIMARY KEY (id))')).toBe(
      'Table: sys_user\nColumns:\n- id · BIGINT\n- name · VARCHAR(64)',
    )
  })
})

describe('MyBatis SQL restore', () => {
  it('restores typed parameters and terminates the SQL statement', () => {
    expect(restoreMyBatisSql(`Preparing:\nSELECT *\nFROM sys_user\nWHERE id = ?\nAND status = ?\n\nParameters:\n1001(Long), 1(Integer)`)).toBe(
      'SELECT *\nFROM sys_user\nWHERE id = 1001\nAND status = 1;',
    )
  })

  it('quotes strings and date-like values, escapes quotes, and renders null', () => {
    expect(restoreMyBatisSql("==> Preparing: SELECT * FROM users WHERE name = ? AND created_at = ? AND deleted_at = ?\n==> Parameters: O'Reilly(String), 2024-01-01(Date), null"))
      .toBe("SELECT * FROM users WHERE name = 'O''Reilly' AND created_at = '2024-01-01' AND deleted_at = NULL;")
  })

  it('rejects parameter count mismatches with a useful error', () => {
    expect(() => restoreMyBatisSql('Preparing: SELECT * FROM users WHERE id = ?\nParameters: 1(Integer), 2(Integer)')).toThrow('expects 1 parameter')
  })
})

function toBase64Url(value: string): string {
  return encodeBase64(value).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '')
}
