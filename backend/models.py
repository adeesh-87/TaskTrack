from sqlalchemy import Column, Integer, String, Text, DateTime
from datetime import datetime
from database import Base

class TaskNote(Base):
    __tablename__ = "task_notes"

    id = Column(Integer, primary_key=True, index=True)
    title = Column(String(100))
    content = Column(Text)  # Markdown text
    tags = Column(String(200), nullable=True)
    created_at = Column(DateTime, default=datetime.utcnow)
