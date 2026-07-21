import { describe, it, expect } from 'vitest'
import {
  emptyToolForm,
  toolFormToPayload,
  toolToFormData,
  type ToolDefinition,
} from '@/types/mcp'

describe('mcp type helpers', () => {
  describe('emptyToolForm', () => {
    it('returns empty form with no selections', () => {
      const form = emptyToolForm()
      expect(form.name).toBe('')
      expect(form.description).toBe('')
      expect(form.selectedRepos.size).toBe(0)
      expect(form.selectedGroups.size).toBe(0)
      expect(form.selectedWebsites.size).toBe(0)
    })
  })

  describe('toolFormToPayload', () => {
    it('converts form to API payload with scopes', () => {
      const form = emptyToolForm()
      form.name = '  my_tool  '
      form.description = ' desc '
      form.selectedRepos = new Set(['repo-1', 'repo-2'])
      form.selectedGroups = new Set(['group-1'])

      const payload = toolFormToPayload(form)
      expect(payload.name).toBe('my_tool')
      expect(payload.description).toBe('desc')
      expect(payload.code_search_scopes).toEqual([{ collection: 'codebase', repo_names: ['repo-1', 'repo-2'] }])
      expect(payload.document_search_scopes).toEqual([{ collection: 'general', group_ids: ['group-1'] }])
    })

    it('returns empty arrays when no selections', () => {
      const payload = toolFormToPayload(emptyToolForm())
      expect(payload.code_search_scopes).toEqual([])
      expect(payload.document_search_scopes).toEqual([])
    })

    it('merges selectedWebsites into group_ids', () => {
      const form = emptyToolForm()
      form.name = 'web_tool'
      form.selectedWebsites = new Set(['docs.example.com', 'blog.example.com'])

      const payload = toolFormToPayload(form)
      expect(payload.document_search_scopes).toEqual([{
        collection: 'general',
        group_ids: ['docs.example.com', 'blog.example.com'],
      }])
    })

    it('deduplicates overlapping group and website selections', () => {
      const form = emptyToolForm()
      form.name = 'dedup'
      form.selectedGroups = new Set(['example.com'])
      form.selectedWebsites = new Set(['example.com'])

      const payload = toolFormToPayload(form)
      expect(payload.document_search_scopes).toEqual([{
        collection: 'general',
        group_ids: ['example.com'],
      }])
    })
  })

  describe('toolToFormData', () => {
    it('extracts form data from a tool definition', () => {
      const tool: ToolDefinition = {
        id: 't1',
        mcp_server_id: 's1',
        name: 'search_code',
        description: 'Find code',
        active: true,
        created_at: '2024-01-01',
        updated_at: '2024-01-01',
        code_search_scopes: [
          { id: 'cs1', tool_definition_id: 't1', collection: 'codebase', repo_names: ['repo-a', 'repo-b'] },
        ],
        document_search_scopes: [
          { id: 'ds1', tool_definition_id: 't1', collection: 'general', group_ids: ['grp-x'] },
        ],
      }

      const form = toolToFormData(tool)
      expect(form.name).toBe('search_code')
      expect(form.description).toBe('Find code')
      expect(form.selectedRepos).toEqual(new Set(['repo-a', 'repo-b']))
      expect(form.selectedGroups).toEqual(new Set(['grp-x']))
      expect(form.selectedWebsites).toEqual(new Set())
    })

    it('handles null description', () => {
      const tool: ToolDefinition = {
        id: 't1', mcp_server_id: 's1', name: 'tool',
        description: null, active: true, created_at: '', updated_at: '',
        code_search_scopes: [], document_search_scopes: [],
      }
      expect(toolToFormData(tool).description).toBe('')
    })

    it('splits website group_ids into selectedWebsites when provided website list', () => {
      const tool: ToolDefinition = {
        id: 't2', mcp_server_id: 's1', name: 'mixed',
        description: '', active: true, created_at: '', updated_at: '',
        code_search_scopes: [],
        document_search_scopes: [
          { id: 'ds1', tool_definition_id: 't2', collection: 'general', group_ids: ['doc-group', 'docs.example.com'] },
        ],
      }
      const form = toolToFormData(tool, ['docs.example.com'])
      expect(form.selectedGroups).toEqual(new Set(['doc-group']))
      expect(form.selectedWebsites).toEqual(new Set(['docs.example.com']))
    })
  })
})
