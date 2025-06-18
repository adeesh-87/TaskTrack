from pydantic import BaseModel
from typing import Optional, List
from datetime import datetime

class TaskNoteCreate(BaseModel):
    title: str
    content: str
    tags: List[str] = []

class TaskNoteRead(BaseModel):
    id: int
    title: str
    content: str
    tags: List[str]
    created_at: datetime

    class Config:
        orm_mode = True


class TaskNoteBase(BaseModel):
    title: str
    content: str
    tags: List[str] = []

    class Config:
        orm_mode = True
