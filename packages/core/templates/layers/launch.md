# Agent Task

You are an AI coding agent. Complete the GitHub issue below.

## Issue: {{ issue.title }}

URL: {{ issue.issue_url }}
State: {{ issue.state }}
Labels: {{ issue.labels | join(sep=", ") }}

## Description

{{ issue.body }}

{% if issue.comments %}
## Recent Comments

{% for comment in issue.comments %}
<comment author="{{ comment.author }}" created_at="{{ comment.created_at }}">
{{ comment.body }}
</comment>

{% endfor %}
{% endif %}

{% if skills %}
## Skills

{% for skill in skills %}
{{ skill }}
{% endfor %}
{% endif %}

{% if user_rules %}
## Rules

{{ user_rules }}
{% endif %}
