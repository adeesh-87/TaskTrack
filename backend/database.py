from sqlalchemy import create_engine
from sqlalchemy.orm import sessionmaker, declarative_base

# DATABASE_URL = "sqlite:///./taskmark.db"
DATABASE_URL = "mysql+pymysql://u421163761_testuser:testPass111!!!!!!!@srv1871.hstgr.io:3306/u421163761_tasktrack"

engine = create_engine(DATABASE_URL)
SessionLocal = sessionmaker(bind=engine, autocommit=False, autoflush=False)
Base = declarative_base()
