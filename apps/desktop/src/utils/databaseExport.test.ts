import { describe, expect, it } from 'vitest'
import type { QueryResult } from '@dev-workbench/shared'
import { queryResultToCsv, queryResultToJson } from './databaseExport'

const result: QueryResult = {
  columns: [{ name: 'id', type: 'INT' }, { name: 'note', type: 'TEXT' }, { name: 'missing', type: 'TEXT' }],
  rows: [{ id: 1, note: 'hello, "world"\nnext', missing: null }],
  executionTime: 1,
}

describe('database result export', () => {
  it('serializes CSV with escaped commas, quotes, line breaks, and empty nulls', () => {
    expect(queryResultToCsv(result)).toBe('id,note,missing\r\n1,"hello, ""world""\nnext",')
  })

  it('serializes JSON as displayed column records and preserves nulls', () => {
    expect(queryResultToJson(result)).toBe('[\n  {\n    "id": 1,\n    "note": "hello, \\\"world\\\"\\nnext",\n    "missing": null\n  }\n]')
  })
})
