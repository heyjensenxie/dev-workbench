import { describe, expect, it } from 'vitest'
import { analyzeSqlSafety, applyResultLimit, splitSqlStatements } from './database'

describe('database SQL helpers', () => {
  it('splits scripts without splitting quoted semicolons', () => {
    expect(splitSqlStatements("SELECT 'a;b'; SELECT 2;")).toEqual(["SELECT 'a;b'", 'SELECT 2'])
  })

  it('requires confirmation for destructive statements and missing predicates', () => {
    expect(analyzeSqlSafety('UPDATE users SET status = 0;').map(({ kind }) => kind)).toEqual(['update-without-where'])
    expect(analyzeSqlSafety('DROP TABLE users;').map(({ kind }) => kind)).toEqual(['drop'])
    expect(analyzeSqlSafety('DELETE FROM users WHERE id = 1')).toEqual([])
  })

  it('reports destructive statements without presentation copy', () => {
    expect(analyzeSqlSafety('TRUNCATE TABLE users;')).toEqual([{ kind: 'truncate' }])
  })

  it('adds a safe result limit only to read queries without one', () => {
    expect(applyResultLimit('SELECT * FROM users', 100)).toContain('LIMIT 100')
    expect(applyResultLimit('SELECT * FROM users LIMIT 10', 100)).toBe('SELECT * FROM users LIMIT 10')
    expect(applyResultLimit('UPDATE users SET status = 0', 100)).toBe('UPDATE users SET status = 0')
  })

  it('does not append LIMIT to metadata statements', () => {
    expect(applyResultLimit('SHOW FULL TABLES FROM `olinker`;', 500)).toBe('SHOW FULL TABLES FROM `olinker`;')
    expect(applyResultLimit('DESCRIBE `sys_user`;', 500)).toBe('DESCRIBE `sys_user`;')
    expect(applyResultLimit('EXPLAIN SELECT * FROM `sys_user`;', 500)).toBe('EXPLAIN SELECT * FROM `sys_user`;')
    expect(applyResultLimit('PRAGMA table_info(projects);', 500)).toBe('PRAGMA table_info(projects);')
  })
})
