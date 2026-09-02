export type PageHelpSection = {
  /** Accordion 的稳定键，同时便于测试精确定位章节。 */
  id: string;
  title: string;
  description?: string;
  steps: readonly string[];
  notes?: readonly string[];
};

export type PageHelpGuide = {
  title: string;
  summary: string;
  recommendedFor: string;
  sections: readonly PageHelpSection[];
};
