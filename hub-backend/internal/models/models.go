package models

type Agent struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	AvatarUrl string `json:"avatarUrl"`
	Status    string `json:"status"`
}

type Message struct {
	ID          string `json:"id"`
	Role        string `json:"role"`
	Content     string `json:"content"`
	Timestamp   string `json:"timestamp"`
	IsEncrypted bool   `json:"isEncrypted"`
}

type SearchResult struct {
	Score       float64 `json:"score"`
	Slug        string  `json:"slug"`
	DisplayName string  `json:"displayName"`
	Summary     string  `json:"summary"`
	Version     string  `json:"version"`
	UpdatedAt   int64   `json:"updatedAt"`
}

type SkillMeta struct {
	Skill         SkillInfo      `json:"skill"`
	LatestVersion VersionInfo    `json:"latestVersion"`
	Moderation    ModerationInfo `json:"moderation"`
}

type SkillInfo struct {
	Slug        string `json:"slug"`
	DisplayName string `json:"displayName"`
	Summary     string `json:"summary"`
	CreatedAt   int64  `json:"createdAt"`
	UpdatedAt   int64  `json:"updatedAt"`
}

type VersionInfo struct {
	Version   string `json:"version"`
	CreatedAt int64  `json:"createdAt"`
	Changelog string `json:"changelog"`
}

type ModerationInfo struct {
	IsSuspicious     bool `json:"isSuspicious"`
	IsMalwareBlocked bool `json:"isMalwareBlocked"`
}
