from pydantic import BaseModel
from typing import Optional, List
from datetime import datetime

class TaskNoteCreate(BaseModel):
    title: str
    content: str
    tags: Optional[str] = None

class TaskNoteRead(TaskNoteCreate):
    id: int
    created_at: datetime

    class Config:
        orm_mode = True

class TaskNoteBase(BaseModel):
    title: str
    content: str
    tags: List[str] = []

    class Config:
        orm_mode = True
